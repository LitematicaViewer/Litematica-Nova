/**
 * 与渲染页九向预设（0..8）对齐，驱动 vscode-nbt StructureEditor 的轨道角 cRot。
 * 与 FOV 滑动条（0..110，0 为正交）同步 cFovDeg。
 * 依赖 editor.js 将主 Editor 实例挂在 window.__lbaNbtEditor。
 */
(function () {
  if (typeof window.__lbaCameraFov !== "number") {
    window.__lbaCameraFov = 70;
  }
  /**
   * StructureEditor.getViewMatrix：T(0,0,-cDist)*Rx(cRot[1])*Ry(cRot[0])*T(cPos)
   * cRot[0]=yaw（绕 Y），cRot[1]=pitch（绕 X），与调试界面一致。
   * 四向：顶视 0/90°、北 180°/0、南 0/0、东 -90°/0、西 90°/0。
   * 四角对角：从结构中心指向各水平象限的体对角方向，yaw=atan2(±sx,±sz)（sx=sz 时与仅用 atan2(z,x)、π/2−… 可区分四向）；pitch=atan2(y,√(x²+z²))。
   */
  function presetToYawPitch(p, se) {
    var sz = se && se.structure && se.structure.getSize ? se.structure.getSize() : [1, 1, 1];
    var sx = Math.max(Number(sz[0]) || 0, 1e-6);
    var sy = Math.max(Number(sz[1]) || 0, 1e-6);
    var sz_ = Math.max(Number(sz[2]) || 0, 1e-6);
    var pitchDiag = Math.atan2(sy, Math.hypot(sx, sz_));
    switch (p | 0) {
      case 0:
        return [0, Math.PI / 2];
      case 1:
        return [Math.PI, 0];
      case 2:
        return [0, 0];
      case 3:
        return [Math.PI / 2, 0];
      case 4:
        return [-Math.PI / 2, 0];
      case 5:
        return [Math.atan2(-sx, -sz_), pitchDiag];
      case 6:
        return [Math.atan2(-sx, sz_), pitchDiag];
      case 7:
        return [Math.atan2(sx, sz_), pitchDiag];
      case 8:
      default:
        return [Math.atan2(sx, -sz_), pitchDiag];
    }
  }

  /** 与 StructureEditor.onInit / ChunkEditor.onInit 一致，并恢复构造默认 cRot（0.4, 0.6）。 */
  function lbaResetStructureLikeCamera(se, ed) {
    if (!se || !se.structure || !se.cPos || !se.cRot) {
      return;
    }
    var sz = se.structure.getSize();
    var sx = Number(sz[0]) || 0;
    var sy = Number(sz[1]) || 0;
    var sz2 = Number(sz[2]) || 0;
    var chunk = ed && ed.type === "chunk";
    if (chunk) {
      se.cPos[0] = sx * -0.5;
      se.cPos[1] = sy * -1 + 16;
      se.cPos[2] = sz2 * -0.5;
      se.cDist = 25;
    } else {
      se.cPos[0] = -0.5 * sx;
      se.cPos[1] = -0.5 * sy;
      se.cPos[2] = -0.5 * sz2;
      se.cDist = Math.hypot(se.cPos[0], se.cPos[1], se.cPos[2]) * 1.5;
    }
    se.cRot[0] = 0.4;
    se.cRot[1] = 0.6;
    if (typeof se.render === "function") {
      se.render();
    }
  }

  /** 与 Editor.type 对应：structure / chunk 均使用 StructureEditor 系 3D（ChunkEditor 继承自 StructureEditor） */
  function lbaStructureLikePanelKey(ed) {
    if (!ed) {
      return null;
    }
    if (ed.type === "chunk") {
      return "chunk";
    }
    if (ed.type === "structure") {
      return "structure";
    }
    return null;
  }

  function lbaGetStructureLikeEditor(ed) {
    var key = lbaStructureLikePanelKey(ed);
    if (!key || !ed.panels || !ed.panels[key]) {
      return null;
    }
    try {
      return ed.panels[key].editor();
    } catch (e) {
      return null;
    }
  }

  window.lbaSetViewPreset = function (n) {
    var ed = window.__lbaNbtEditor;
    var se = lbaGetStructureLikeEditor(ed);
    if (!se || !se.cRot) {
      return;
    }
    var p = Math.max(0, Math.min(8, n | 0));
    var yp = presetToYawPitch(p, se);
    se.cRot[0] = yp[0];
    se.cRot[1] = yp[1];
    if (typeof se.render === "function") {
      se.render();
    }
  };

  window.lbaSetCameraFov = function (deg) {
    var v = Math.max(0, Math.min(110, deg | 0));
    window.__lbaCameraFov = v;
    var ed = window.__lbaNbtEditor;
    var se = lbaGetStructureLikeEditor(ed);
    if (!se) {
      return;
    }
    se.cFovDeg = v;
    if (typeof se.render === "function") {
      se.render();
    }
  };

  window.lbaResetNbtDefaultCamera = function () {
    var ed = window.__lbaNbtEditor;
    var se = lbaGetStructureLikeEditor(ed);
    lbaResetStructureLikeCamera(se, ed);
  };
})();
