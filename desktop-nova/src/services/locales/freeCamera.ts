// One binding description for both input methods and both UI languages.
const cameraKeys = { movement: "WASD", up: "Space", down: "Shift", boost: "Ctrl", release: "Esc" } as const;
const zhKeys = `${cameraKeys.movement} 移动，${cameraKeys.up} 上升，${cameraKeys.down} 下降，${cameraKeys.boost} 加速；${cameraKeys.release} 释放。`;
const enKeys = `${cameraKeys.movement} move, ${cameraKeys.up} up, ${cameraKeys.down} down, ${cameraKeys.boost} boost; ${cameraKeys.release} release.`;
export const zhCN: Record<string, string> = {
  "freeCamera.title": "相机操作", "freeCamera.speed": "移动速度", "freeCamera.speedUnit": "格/秒",
  "freeCamera.sensitivity": "鼠标灵敏度", "freeCamera.reset": "重置视角",
  "freeCamera.inputMode": "鼠标操作", "freeCamera.drag": "鼠标拖拽", "freeCamera.pointerLock": "点击锁定鼠标",
  "freeCamera.dragHint": `右键按住转向，松开结束。${zhKeys}`,
  "freeCamera.lockHint": `点击视口锁定并隐藏鼠标，移动鼠标转向。${zhKeys}`,
};
export const enUS: Record<string, string> = {
  "freeCamera.title": "Camera controls", "freeCamera.speed": "Move speed", "freeCamera.speedUnit": "blocks/s",
  "freeCamera.sensitivity": "Mouse sensitivity", "freeCamera.reset": "Reset view",
  "freeCamera.inputMode": "Mouse controls", "freeCamera.drag": "Mouse drag", "freeCamera.pointerLock": "Click to lock pointer",
  "freeCamera.dragHint": `Hold right mouse to look, release to stop. ${enKeys}`,
  "freeCamera.lockHint": `Click the viewport to lock and hide the pointer; move to look. ${enKeys}`,
};
