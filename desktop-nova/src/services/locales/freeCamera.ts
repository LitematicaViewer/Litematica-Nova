// One binding description for both input methods and both UI languages.
const cameraKeys = { movement: "WASD", up: "Space", down: "Shift", boost: "Ctrl", release: "Esc" } as const;
const zhKeys = `${cameraKeys.movement} 移动，${cameraKeys.up} 上升，${cameraKeys.down} 下降，${cameraKeys.boost} 加速；${cameraKeys.release} 释放。`;
const enKeys = `${cameraKeys.movement} move, ${cameraKeys.up} up, ${cameraKeys.down} down, ${cameraKeys.boost} boost; ${cameraKeys.release} release.`;
export const zhCN: Record<string, string> = {
  "freeCamera.title": "相机操作", "freeCamera.speed": "移动速度", "freeCamera.speedUnit": "格/秒",
  "freeCamera.sensitivity": "鼠标灵敏度", "freeCamera.reset": "重置视角",
  "freeCamera.inputMode": "鼠标操作", "freeCamera.drag": "鼠标拖拽", "freeCamera.pointerLock": "点击锁定鼠标",
  "freeCamera.dragHint": "左键围绕投影旋转，右键平移，滚轮朝指针缩放；编辑时 Alt＋左键临时观察。Esc 释放。",
  "freeCamera.locked": "鼠标已锁定", "freeCamera.unlocked": "鼠标未锁定",
  "environment.title": "环境", "environment.time": "时间", "environment.ticks": "ticks",
  "environment.weather": "天气", "environment.clockHint": "0＝日出 06:00 · 6000＝正午 12:00 · 12000＝日落 18:00 · 18000＝午夜 00:00",
  "environment.weather.auto": "跟随世界", "environment.weather.clear": "晴", "environment.weather.rain": "雨",
  "environment.weather.thunder": "雷暴", "environment.weather.snow": "雪",
  "environment.unavailable": "Fast / Unlit 没有真实环境效果；请使用支持环境的 Full 渲染。",
  "environment.permissionHint": "部分控制已由设置中的环境修改权限关闭。",
  "environment.autoHint": "跟随世界使用文件可用的数据；投影未提供生物群系时不会模拟雨雪。",
  "environment.applying": "正在应用最新选择…", "environment.applied": "Viewer 已确认当前环境",
  "environment.waiting": "等待 Viewer 确认环境状态", "environment.failed": "应用失败，请检查下方原因",
  "freeCamera.lockHint": `点击视口锁定并隐藏鼠标，移动鼠标转向。${zhKeys}`,
};
export const enUS: Record<string, string> = {
  "freeCamera.title": "Camera controls", "freeCamera.speed": "Move speed", "freeCamera.speedUnit": "blocks/s",
  "freeCamera.sensitivity": "Mouse sensitivity", "freeCamera.reset": "Reset view",
  "freeCamera.inputMode": "Mouse controls", "freeCamera.drag": "Mouse drag", "freeCamera.pointerLock": "Click to lock pointer",
  "freeCamera.dragHint": "Left-drag to orbit the model, right-drag to pan, wheel to zoom toward the pointer. Alt+left-drag temporarily observes while editing. Esc releases input.",
  "freeCamera.locked": "Pointer locked", "freeCamera.unlocked": "Pointer not locked",
  "environment.title": "Environment", "environment.time": "Time", "environment.ticks": "ticks",
  "environment.weather": "Weather", "environment.clockHint": "0 = sunrise 06:00 · 6000 = noon 12:00 · 12000 = sunset 18:00 · 18000 = midnight 00:00",
  "environment.weather.auto": "Follow world", "environment.weather.clear": "Clear", "environment.weather.rain": "Rain",
  "environment.weather.thunder": "Thunderstorm", "environment.weather.snow": "Snow",
  "environment.unavailable": "Fast / Unlit has no live environment effects. Use a Full renderer that supports them.",
  "environment.permissionHint": "Some controls are disabled by the environment permissions in settings.",
  "environment.autoHint": "Follow world uses the data available in the file. Rain and snow are not simulated when biome data is absent.",
  "environment.applying": "Applying the latest choice…", "environment.applied": "Viewer confirmed the current environment",
  "environment.waiting": "Waiting for the Viewer environment state", "environment.failed": "Change failed; see the reason below",
  "freeCamera.lockHint": `Click the viewport to lock and hide the pointer; move to look. ${enKeys}`,
};
