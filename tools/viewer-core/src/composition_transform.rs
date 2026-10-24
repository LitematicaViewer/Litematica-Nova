//! Integer grid transforms shared by composition, split and format consumers.
use crate::model::BlockStateNbt;
use anyhow::{Result, bail, ensure};
use fastnbt::Value;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Transform {
    pub offset: [i32; 3],
    pub origin: [i32; 3],
    pub rotation: u16,
    pub mirror_x: bool,
    pub mirror_z: bool,
}
impl Transform {
    pub fn validate(self) -> Result<Self> {
        ensure!(
            matches!(self.rotation, 0 | 90 | 180 | 270),
            "旋转只能为0/90/180/270度"
        );
        Ok(self)
    }
    pub fn is_oriented(self) -> bool {
        self.rotation != 0 || self.mirror_x || self.mirror_z
    }
    fn direction(self, mut p: [f64; 3]) -> [f64; 3] {
        if self.mirror_x {
            p[0] = -p[0]
        }
        if self.mirror_z {
            p[2] = -p[2]
        }
        for _ in 0..self.rotation / 90 {
            p = [-p[2], p[1], p[0]]
        }
        p
    }
    /// Block centers and floating entity positions share one affine transform.
    pub fn point(self, p: [f64; 3]) -> [f64; 3] {
        let q = self.direction(std::array::from_fn(|a| p[a] - f64::from(self.origin[a])));
        std::array::from_fn(|a| q[a] + f64::from(self.origin[a]) + f64::from(self.offset[a]))
    }
    pub fn block(self, p: [i32; 3]) -> Result<[i32; 3]> {
        let q = self.point(p.map(|v| f64::from(v) + 0.5));
        let mut r = [0; 3];
        for a in 0..3 {
            let v = q[a].floor();
            ensure!(
                v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX),
                "变换坐标超出整数范围"
            );
            r[a] = v as i32
        }
        Ok(r)
    }
    pub fn facing(self, name: &str) -> Result<String> {
        let p = match name {
            "north" => [0., 0., -1.],
            "south" => [0., 0., 1.],
            "west" => [-1., 0., 0.],
            "east" => [1., 0., 0.],
            "up" | "down" => return Ok(name.into()),
            _ => bail!("不支持的朝向字段:{name}"),
        };
        let p = self.direction(p);
        Ok(if p[0] < -0.5 {
            "west"
        } else if p[0] > 0.5 {
            "east"
        } else if p[2] < -0.5 {
            "north"
        } else {
            "south"
        }
        .into())
    }
    pub fn yaw(self, yaw: f64) -> f64 {
        let r = yaw.to_radians();
        let p = self.direction([-r.sin(), 0., r.cos()]);
        (-p[0]).atan2(p[2]).to_degrees()
    }
    pub fn state(self, state: &BlockStateNbt) -> Result<BlockStateNbt> {
        if !self.is_oriented() {
            return Ok(state.clone());
        }
        ensure!(
            state.name.starts_with("minecraft:"),
            "{}的模组方向语义尚未提供等价变换",
            state.name
        );
        let mut result = state.clone();
        let mut properties = BTreeMap::new();
        for (key, value) in &state.properties {
            let out_key = if matches!(key.as_str(), "north" | "east" | "south" | "west") {
                self.facing(key)?
            } else {
                key.clone()
            };
            let out_value = match key.as_str() {
                "facing" | "horizontal_facing" => self.facing(value)?,
                "axis" if value == "x" || value == "z" => {
                    if self.rotation % 180 == 90 {
                        if value == "x" { "z" } else { "x" }.into()
                    } else {
                        value.clone()
                    }
                }
                "rotation" => {
                    let v = value.parse::<u16>()?;
                    ensure!(v < 16, "16向旋转越界");
                    ((self.yaw(f64::from(v) * 22.5) / 22.5).round() as i32)
                        .rem_euclid(16)
                        .to_string()
                }
                "shape" if state.name.ends_with("rail") => self.rail(value)?,
                "shape" | "hinge" | "type" if self.mirror_x ^ self.mirror_z => match value.as_str()
                {
                    "left" => "right".into(),
                    "right" => "left".into(),
                    "inner_left" => "inner_right".into(),
                    "inner_right" => "inner_left".into(),
                    "outer_left" => "outer_right".into(),
                    "outer_right" => "outer_left".into(),
                    _ => value.clone(),
                },
                "orientation" => {
                    let parts = value
                        .split('_')
                        .map(|p| self.facing(p))
                        .collect::<Result<Vec<_>>>()?;
                    parts.join("_")
                }
                _ => value.clone(),
            };
            properties.insert(out_key, out_value);
        }
        result.properties = properties;
        Ok(result)
    }
    fn rail(self, value: &str) -> Result<String> {
        if let Some(v) = value.strip_prefix("ascending_") {
            return Ok(format!("ascending_{}", self.facing(v)?));
        }
        let p = value
            .split('_')
            .map(|v| self.facing(v))
            .collect::<Result<Vec<_>>>()?;
        ensure!(p.len() == 2, "轨道形状无效");
        for (a, b) in [
            ("north", "south"),
            ("east", "west"),
            ("south", "east"),
            ("south", "west"),
            ("north", "west"),
            ("north", "east"),
        ] {
            if p.contains(&a.into()) && p.contains(&b.into()) {
                return Ok(format!("{a}_{b}"));
            }
        }
        bail!("轨道形状无法等价变换:{value}")
    }
    pub fn entity(self, value: &mut Value, from: [i32; 3], to: [i32; 3]) -> Result<()> {
        let Value::Compound(map) = value else {
            bail!("实体NBT不是Compound")
        };
        if let Some(Value::List(pos)) = map.get_mut("Pos") {
            ensure!(pos.len() == 3, "实体Pos字段无效");
            let mut p = [0.; 3];
            for a in 0..3 {
                p[a] = number(&pos[a])? + f64::from(from[a]);
            }
            let p = self.point(p);
            for a in 0..3 {
                pos[a] = Value::Double(p[a] - f64::from(to[a]));
            }
        } else {
            bail!("实体缺少明确的Pos锚点")
        }
        if let Some(Value::List(rotation)) = map.get_mut("Rotation") {
            ensure!(rotation.len() == 2, "实体Rotation字段无效");
            rotation[0] = Value::Float(self.yaw(number(&rotation[0])?) as f32);
        }
        if let Some(Value::List(motion)) = map.get_mut("Motion") {
            ensure!(motion.len() == 3, "实体Motion字段无效");
            let p = self.direction([
                number(&motion[0])?,
                number(&motion[1])?,
                number(&motion[2])?,
            ]);
            for a in 0..3 {
                motion[a] = Value::Double(p[a]);
            }
        }
        self.anchors(map, from, to)?;
        if let Some(Value::IntArray(delta)) = map.get_mut("NovaStructureAnchorDelta") {
            ensure!(delta.len() == 3, "Structure实体锚点偏移无效");
            let v = self.direction([
                f64::from(delta[0]),
                f64::from(delta[1]),
                f64::from(delta[2]),
            ]);
            *delta = fastnbt::IntArray::new(v.map(|v| v.round() as i32).to_vec());
        }
        if let Some(Value::List(passengers)) = map.get_mut("Passengers") {
            for passenger in passengers {
                self.entity(passenger, from, to)?
            }
        }
        Ok(())
    }
    pub fn block_entity(self, value: &mut Value, from: [i32; 3], to: [i32; 3]) -> Result<()> {
        let Value::Compound(map) = value else {
            bail!("方块实体NBT不是Compound")
        };
        self.anchors(map, from, to)?;
        if let Some(Value::IntArray(pos)) = map.get_mut("pos") {
            ensure!(pos.len() == 3, "计划刻pos坐标长度无效");
            let mut p = [0; 3];
            for a in 0..3 {
                p[a] = pos[a]
                    .checked_add(from[a])
                    .ok_or_else(|| anyhow::anyhow!("计划刻坐标溢出"))?;
            }
            let p = self.block(p)?;
            let mut local = [0; 3];
            for a in 0..3 {
                local[a] = p[a]
                    .checked_sub(to[a])
                    .ok_or_else(|| anyhow::anyhow!("计划刻坐标溢出"))?;
            }
            *pos = fastnbt::IntArray::new(local.to_vec());
        }
        for key in [
            "ExitPortal",
            "exit_portal",
            "FlowerPos",
            "flower_pos",
            "HivePos",
            "hive_pos",
        ] {
            if let Some(value) = map.get_mut(key) {
                match value {
                    Value::Compound(pos) => self.anchors(pos, from, to)?,
                    Value::IntArray(pos) if pos.len() == 3 => {
                        let mut absolute = [0; 3];
                        for a in 0..3 {
                            absolute[a] = pos[a]
                                .checked_add(from[a])
                                .ok_or_else(|| anyhow::anyhow!("方块实体{key}坐标溢出"))?;
                        }
                        let p = self.block(absolute)?;
                        let mut local = [0; 3];
                        for a in 0..3 {
                            local[a] = p[a]
                                .checked_sub(to[a])
                                .ok_or_else(|| anyhow::anyhow!("方块实体{key}坐标溢出"))?;
                        }
                        *pos = fastnbt::IntArray::new(local.to_vec());
                    }
                    _ => bail!("方块实体{key}位置字段不受支持"),
                }
            }
        }
        Ok(())
    }
    fn anchors(self, map: &mut HashMap<String, Value>, from: [i32; 3], to: [i32; 3]) -> Result<()> {
        for keys in [
            ["x", "y", "z"],
            ["TileX", "TileY", "TileZ"],
            ["SleepingX", "SleepingY", "SleepingZ"],
            ["X", "Y", "Z"],
            ["HomePosX", "HomePosY", "HomePosZ"],
            ["TravelPosX", "TravelPosY", "TravelPosZ"],
            ["BoundX", "BoundY", "BoundZ"],
            ["APX", "APY", "APZ"],
        ] {
            if keys.iter().any(|k| map.contains_key(*k)) {
                let mut p = [0; 3];
                for a in 0..3 {
                    p[a] = i32::try_from(integer(
                        map.get(keys[a])
                            .ok_or_else(|| anyhow::anyhow!("不完整的位置字段"))?,
                    )?)?
                    .checked_add(from[a])
                    .ok_or_else(|| anyhow::anyhow!("NBT坐标溢出"))?;
                }
                let p = self.block(p)?;
                for a in 0..3 {
                    map.insert(
                        keys[a].into(),
                        Value::Int(
                            p[a].checked_sub(to[a])
                                .ok_or_else(|| anyhow::anyhow!("NBT坐标溢出"))?,
                        ),
                    );
                }
            }
        }
        if self.is_oriented() {
            for key in ["facing", "Facing"] {
                if let Some(v) = map.get_mut(key) {
                    let n = integer(v)?;
                    let names = ["down", "up", "north", "south", "west", "east"];
                    ensure!((0..6).contains(&n), "NBT Facing字段无效");
                    let name = self.facing(names[n as usize])?;
                    let out = names.iter().position(|v| *v == name).unwrap() as i64;
                    *v = match v {
                        Value::Byte(_) => Value::Byte(out as i8),
                        Value::Short(_) => Value::Short(out as i16),
                        _ => Value::Int(out as i32),
                    };
                }
            }
            for key in ["Rot", "SkullRotation"] {
                if let Some(v) = map.get_mut(key) {
                    let n = integer(v)?;
                    ensure!((0..16).contains(&n), "NBT旋转字段无效");
                    let out = ((self.yaw(n as f64 * 22.5) / 22.5).round() as i32).rem_euclid(16);
                    *v = Value::Byte(out as i8);
                }
            }
        }
        Ok(())
    }
}
pub fn number(v: &Value) -> Result<f64> {
    Ok(match v {
        Value::Byte(v) => f64::from(*v),
        Value::Short(v) => f64::from(*v),
        Value::Int(v) => f64::from(*v),
        Value::Long(v) => *v as f64,
        Value::Float(v) => f64::from(*v),
        Value::Double(v) => *v,
        _ => bail!("NBT坐标不是数值"),
    })
}
pub fn integer(v: &Value) -> Result<i64> {
    match v {
        Value::Byte(v) => Ok(i64::from(*v)),
        Value::Short(v) => Ok(i64::from(*v)),
        Value::Int(v) => Ok(i64::from(*v)),
        Value::Long(v) => Ok(*v),
        _ => bail!("NBT方块坐标不是整数"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_and_entities_use_same_affine_transform() {
        let t = Transform {
            rotation: 90,
            ..Default::default()
        };
        assert_eq!(t.block([0, 0, 0]).unwrap(), [-1, 0, 0]);
        assert_eq!(t.point([0.5, 0.5, 0.5]), [-0.5, 0.5, 0.5]);
        assert_eq!(t.facing("north").unwrap(), "east");
    }
    #[test]
    fn mirrors_swap_stair_and_chest_handedness() {
        let s = BlockStateNbt {
            name: "minecraft:oak_stairs".into(),
            properties: BTreeMap::from([
                ("facing".into(), "east".into()),
                ("shape".into(), "inner_left".into()),
            ]),
        };
        let t = Transform {
            mirror_x: true,
            ..Default::default()
        };
        let s = t.state(&s).unwrap();
        assert_eq!(s.properties["facing"], "west");
        assert_eq!(s.properties["shape"], "inner_right");
    }
    #[test]
    fn anvil_ticks_use_the_same_block_transform_and_keep_workload_fields() {
        let mut tick = Value::Compound(HashMap::from([
            (
                "pos".into(),
                Value::IntArray(fastnbt::IntArray::new(vec![1, 2, -3])),
            ),
            ("delay".into(), Value::Int(17)),
            ("fluid".into(), Value::Byte(1)),
        ]));
        let transform = Transform {
            rotation: 90,
            offset: [-16, 0, 32],
            ..Default::default()
        };
        transform.block_entity(&mut tick, [0; 3], [0; 3]).unwrap();
        let Value::Compound(fields) = tick else {
            panic!()
        };
        assert_eq!(
            fields["pos"],
            Value::IntArray(fastnbt::IntArray::new(
                transform.block([1, 2, -3]).unwrap().to_vec()
            ))
        );
        assert_eq!(fields["delay"], Value::Int(17));
        assert_eq!(fields["fluid"], Value::Byte(1));
    }
}
