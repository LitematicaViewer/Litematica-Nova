import json
import argparse
from pathlib import Path

KEY_MAP = {
    "facing": "朝向",
    "axis": "轴向",
    "horizontal_axis": "水平轴向",
    "orientation": "朝向",
    "rotation": "旋转",
    "half": "半部",
    "type": "类型",
    "shape": "形状",
    "waterlogged": "含水",
    "open": "打开",
    "powered": "充能",
    "lit": "点亮",
    "level": "等级",
    "age": "生长阶段",
    "stage": "阶段",
    "distance": "距离",
    "persistent": "持久",
    "snowy": "覆雪",
    "enabled": "启用",
    "occupied": "占用",
    "attached": "附着",
    "hanging": "悬挂",
    "in_wall": "位于墙中",
    "unstable": "不稳定",
    "extended": "伸出",
    "short": "短型",
    "delay": "延迟",
    "locked": "锁定",
    "mode": "模式",
    "conditional": "条件型",
    "triggered": "已触发",
    "disarmed": "已解除",
    "eye": "有末影眼",
    "eggs": "蛋数量",
    "hatch": "孵化阶段",
    "bites": "咬食次数",
    "candles": "蜡烛数量",
    "pickles": "海泡菜数量",
    "layers": "层数",
    "charges": "充能数",
    "power": "红石强度",
    "note": "音符",
    "instrument": "乐器",
    "moisture": "湿度",
    "thickness": "厚度",
    "tilt": "倾斜",
    "sculk_sensor_phase": "幽匿感测阶段",
    "trial_spawner_state": "试炼刷怪笼状态",
    "vault_state": "宝库状态",
    "cracked": "裂纹",
    "crafting": "合成中",
    "has_book": "有书",
    "inverted": "反转",
    "signal_fire": "信号火",
    "bloom": "盛开",
    "drag": "阻力",
    "can_summon": "可召唤",
    "bottom": "下方连接",
    "top": "上方连接",
    "north": "北侧",
    "south": "南侧",
    "east": "东侧",
    "west": "西侧",
    "up": "上侧",
    "down": "下侧",
    "face": "附着面",
    "attachment": "附着方式",
    "hinge": "合页位置",
    "part": "部分",
    "leaves": "树叶",
    "honey_level": "蜂蜜等级",
    "berries": "浆果",
    "has_bottle_0": "有药水瓶_0",
    "has_bottle_1": "有药水瓶_1",
    "has_bottle_2": "有药水瓶_2",
    "has_record": "有唱片",
    "creaking_heart_state": "嘎吱心状态",
    "dusted": "掸扫程度",
    "flower_amount": "花数量",
    "hydration": "湿润度",
    "map": "有地图",
    "ominous": "不祥",
    "segment_amount": "节数量",
    "side_chain": "侧边链",
    "slot_0_occupied": "槽位_0_占用",
    "slot_1_occupied": "槽位_1_占用",
    "slot_2_occupied": "槽位_2_占用",
    "slot_3_occupied": "槽位_3_占用",
    "slot_4_occupied": "槽位_4_占用",
    "slot_5_occupied": "槽位_5_占用",
    "tip": "顶端",
    "vertical_direction": "垂直方向",
}

GLOBAL_VALUE_MAP = {
    "true": "是",
    "false": "否",
    "north": "北",
    "south": "南",
    "east": "东",
    "west": "西",
    "up": "上",
    "down": "下",
    "x": "X轴",
    "y": "Y轴",
    "z": "Z轴",
    "none": "无",
    "low": "低",
    "tall": "高",
    "straight": "直线",
    "inner_left": "内左",
    "inner_right": "内右",
    "outer_left": "外左",
    "outer_right": "外右",
    "north_east": "东北",
    "south_west": "西南",
    "ascending_north": "向北上升",
    "ascending_south": "向南上升",
    "ascending_east": "向东上升",
    "ascending_west": "向西上升",
    "north_south": "南北",
    "east_west": "东西",
    "side": "侧面",
    "floor": "地面",
    "wall": "墙面",
    "ceiling": "天花板",
    "compare": "比较",
    "subtract": "减法",
    "save": "保存",
    "load": "加载",
    "corner": "角落",
    "data": "数据",
    "head": "头部",
    "foot": "脚部",
    "tip": "尖端",
    "tip_merge": "尖端合并",
    "frustum": "锥台",
    "middle": "中段",
    "base": "底座",
    "inactive": "未激活",
    "active": "激活",
    "cooldown": "冷却",
    "ominous": "不祥",
    "ejecting": "弹出中",
    "unlocking": "解锁中",
    "ejecting_reward": "弹出奖励",
    "waiting_for_players": "等待玩家",
    "waiting_for_reward_ejection": "等待奖励弹出",
    "accept": "接受",
    "fail": "失败",
    "log": "记录",
    "start": "开始",
    "awake": "唤醒",
    "dormant": "休眠",
    "uprooted": "拔起",
    "north_up": "北上",
    "north_west": "西北",
    "east_up": "东上",
    "east_south": "东南",
    "south_up": "南上",
    "south_east": "东南",
    "west_up": "西上",
    "west_north": "西北",
    "up_east": "上东",
    "up_north": "上北",
    "up_south": "上南",
    "up_west": "上西",
    "down_east": "下东",
    "down_north": "下北",
    "down_south": "下南",
    "down_west": "下西",
    "left": "左",
    "right": "右",
    "single_wall": "单面墙",
    "double_wall": "双面墙",
    "large": "大",
    "small": "小",
    "full": "完全",
    "partial": "部分",
    "unstable": "不稳定",
    "front": "前",
    "back": "后",
    "side_chain": "侧边链",
    "center": "中心",
    "unconnected": "未连接"
}

VALUE_MAP_OVERRIDES = {
    "type": {
        "bottom": "下半",
        "top": "上半",
        "double": "双层",
        "single": "单层",
        "left": "左",
        "right": "右",
        "normal": "普通",
        "sticky": "粘性"
    },
    "half": {
        "bottom": "下半",
        "top": "上半",
        "lower": "下半",
        "upper": "上半"
    }
}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", type=str, required=True, help="Input blockstate DB JSON")
    parser.add_argument("--output", type=str, required=True, help="Output zh_cn JSON")
    parser.add_argument("--report", type=str, required=True, help="Output missing report JSON")
    args = parser.parse_args()

    db_path = Path(args.db)
    output_path = Path(args.output)
    report_path = Path(args.report)

    if not db_path.is_file():
        print(f"Error: DB file {db_path} not found.")
        return

    with open(db_path, "r", encoding="utf-8") as f:
        db = json.load(f)

    blocks = db.get("blocks", {})
    
    all_keys = set()
    all_key_values = {}

    for block_id, b_data in blocks.items():
        props = b_data.get("properties", {})
        for k, v_list in props.items():
            all_keys.add(k)
            if k not in all_key_values:
                all_key_values[k] = set()
            for v in v_list:
                all_key_values[k].add(v)

    # Build translation
    translated_keys = {}
    translated_values = {}
    missing_keys = []
    missing_values = {}

    total_key_values = 0
    translated_key_values_count = 0

    for k in sorted(all_keys):
        if k in KEY_MAP:
            translated_keys[k] = KEY_MAP[k]
        else:
            missing_keys.append(k)

        translated_values[k] = {}
        missing_values[k] = []
        
        for v in sorted(all_key_values[k]):
            total_key_values += 1
            
            # Numeric check
            if v.isdigit() or (v.startswith('-') and v[1:].isdigit()):
                translated_values[k][v] = v
                translated_key_values_count += 1
                continue
                
            # Override check
            if k in VALUE_MAP_OVERRIDES and v in VALUE_MAP_OVERRIDES[k]:
                translated_values[k][v] = VALUE_MAP_OVERRIDES[k][v]
                translated_key_values_count += 1
                continue
                
            # Global check
            if v in GLOBAL_VALUE_MAP:
                translated_values[k][v] = GLOBAL_VALUE_MAP[v]
                translated_key_values_count += 1
                continue
                
            missing_values[k].append(v)
            
        if not missing_values[k]:
            del missing_values[k]

    out_data = {
        "version_id": db.get("version_id", "unknown"),
        "source_db": str(db_path.as_posix()),
        "locale": "zh_cn",
        "property_keys": translated_keys,
        "property_values": translated_values,
        "fallback": {
            "unknown_key_policy": "show_raw_key",
            "unknown_value_policy": "show_raw_value"
        }
    }

    report_data = {
        "missing_property_keys": missing_keys,
        "missing_property_values": missing_values,
        "coverage": {
            "total_keys": len(all_keys),
            "translated_keys": len(translated_keys),
            "total_key_values": total_key_values,
            "translated_key_values": translated_key_values_count
        }
    }

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(out_data, f, ensure_ascii=False, indent=2, sort_keys=True)

    report_path.parent.mkdir(parents=True, exist_ok=True)
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report_data, f, ensure_ascii=False, indent=2, sort_keys=True)

    print(f"Generated i18n mapping: {output_path}")
    print(f"Generated missing report: {report_path}")
    print(f"Key Coverage: {len(translated_keys)}/{len(all_keys)}")
    print(f"Value Coverage: {translated_key_values_count}/{total_key_values}")

if __name__ == "__main__":
    main()
