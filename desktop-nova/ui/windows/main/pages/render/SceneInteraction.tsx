import { DropdownMenu } from "radix-ui";
import { MoreHorizontal } from "lucide-react";
import { createPortal } from "react-dom";
import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { getSceneViewState, updateSceneView, type SceneViewAck, type SceneViewControls, type SceneInteractionSnapshot, type SceneBlockState } from "../../../../../src/platform/sceneView";
import { saveDialog } from "../../../../../src/platform/dialogs";
import { nativeClientPlacement } from "../../embeddedViewerGeometry";
import { translateBlockId } from "../../../../../src/business/facade";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Button } from "../../../../components/ui/button";
import { Dialog } from "../../../../components/Dialog";
import { CreativeInventoryDialog, useCreativeInventoryCatalog } from "../flake/creativeInventoryDialog";
import { ContainerSlots, SceneInfoHud, type SceneHit } from "./SceneInfoHud";
import "./sceneInteraction.css";

// Viewer snapshots are polled and survive page changes. Consume each physical
// gesture once, even if the toolbar is unmounted and later returns to this scene.
const consumedDetails = new Map<string, number>();

export function SceneInteraction({ sourcePath, operationId, displayMode, ready, visible, blocked, viewportRef, toolbarTarget, onError }: {
  sourcePath: string; operationId: string | undefined; displayMode: "standard" | "full"; ready: boolean; visible: boolean; blocked: boolean;
  viewportRef: React.RefObject<HTMLDivElement | null>; toolbarTarget?: HTMLElement | null;
  onError: (error: string) => void;
}) {
  useI18n();
  const identity = `${operationId}\n${displayMode}\n${sourcePath}`;
  const current = useRef(identity); current.current = identity;
  const [snapshot, setSnapshot] = useState<SceneInteractionSnapshot | null>(null);
  const snapshotRef = useRef(snapshot); snapshotRef.current = snapshot;
  const [error, setError] = useState("");
  const [details, setDetails] = useState<SceneHit | null>(null);
  const [pending, setPending] = useState(false);
  const [systemDialogOpen, setSystemDialogOpen] = useState(false);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const issued = useRef(0);
  const active = !!operationId && ready && !blocked;
  const catalog = useCreativeInventoryCatalog({ version: snapshot?.gameVersion ?? undefined,
    contentId: snapshot?.contentId ?? undefined, enabled: active && !!snapshot });
  const activeRef = useRef(active); activeRef.current = active;
  const inputLease = useRef<symbol | null>(null);
  useLayoutEffect(() => {
    inputLease.current = active ? Symbol(identity) : null;
    return () => { inputLease.current = null; };
  }, [active, identity]);
  const receive = useCallback((ack: SceneViewAck) => {
    if (current.current !== identity || !activeRef.current) return;
    if (ack.error) setError(ack.error);
    const value = ack.interaction;
    if (!value) { setError(t("scene.runtimeMissing")); return; }
    setSnapshot(previous => previous?.sceneGeneration === value.sceneGeneration && previous.revision > value.revision
      || JSON.stringify(previous) === JSON.stringify(value) ? previous : value);
    if (value.error) setError(value.error);
  }, [identity]);
  useEffect(() => { if (error) onError(error); }, [error, onError]);
  const send = useCallback((controls: SceneViewControls) => {
    const ticket = ++issued.current;
    const lease = inputLease.current;
    const task = queue.current.catch(() => undefined).then(async () => {
      if (!lease || lease !== inputLease.current || current.current !== identity || !activeRef.current) return;
      if (controls.uiPointer && (document.hidden || !viewportRef.current ||
        [...document.querySelectorAll("[data-native-ui-blocking]:not([data-state=closed])")].some(el => !el.contains(viewportRef.current)))) return;
      const ack = await updateSceneView({ sourcePath, displayMode, operationId, controls });
      if (ticket === issued.current) receive(ack);
      if (ack.error || ack.interaction?.error) throw new Error(ack.error || ack.interaction?.error || undefined);
    });
    queue.current = task;
    void task.catch(reason => { if (current.current === identity) setError(String(reason)); });
    return task;
  }, [sourcePath, operationId, displayMode, identity, receive, viewportRef]);
  useEffect(() => {
    setSnapshot(null); setDetails(null); setError("");
    if (!active) return;
    let alive = true, querying = false;
    const query = async () => {
      if (!alive || document.hidden || querying) return;
      querying = true;
      const ticket = issued.current;
      try { const ack = await getSceneViewState({ sourcePath, displayMode }); if (alive && ticket === issued.current) receive(ack); }
      catch (reason) { if (alive) setError(String(reason)); }
      finally { querying = false; }
    };
    void query(); const timer = window.setInterval(() => void query(), 250);
    return () => {
      alive = false; window.clearInterval(timer);
      // Presentation cleanup only. The scene, draft and GPU allocations remain owned by Viewer.
      void updateSceneView({ sourcePath, displayMode, operationId, controls: { releaseCameraInput: true, interaction: { menuOpen: false } } }).catch(() => undefined);
    };
  }, [identity, active, sourcePath, displayMode, operationId, receive]);
  const menuOpen = !!snapshot?.menuOpen;
  useEffect(() => {
    const request = snapshot?.detailsRequest;
    if (!snapshot || !request) return;
    const scene = `${identity}\n${snapshot.sceneGeneration}`;
    if (request.sequence <= (consumedDetails.get(scene) ?? -1)) return;
    consumedDetails.set(scene, request.sequence);
    if (consumedDetails.size > 64) consumedDetails.delete(consumedDetails.keys().next().value!);
    if (active && visible && !menuOpen && !snapshot.pointerLocked && snapshot.cameraInputMode !== "pointer_lock") {
      setDetails(request.hit);
      void send({ releaseCameraInput: true }).catch(() => undefined);
    }
  }, [identity, snapshot, active, visible, menuOpen, send]);
  const closeMenu = useCallback(() => { void send({ interaction: { menuOpen: false }, releaseCameraInput: true }); }, [send]);
  useEffect(() => {
    if (!active) return;
    const key = (event: KeyboardEvent) => {
      const input = event.target instanceof Element && event.target.closest('input,textarea,select,[contenteditable="true"]');
      if (event.isComposing || event.defaultPrevented || input || event.repeat || event.ctrlKey || event.metaKey || event.altKey) return;
      // Radix gives Escape to the topmost Dialog. A background inventory must
      // neither consume it nor react to E typed in a newer confirmation dialog.
      const dialog = event.target instanceof Element && event.target.closest('[role="dialog"]');
      if (dialog && !dialog.matches(".flake-page__inventory-dialog")) return;
      if (menuOpen && event.code === "KeyE") { event.preventDefault(); closeMenu(); return; }
      if (menuOpen || details || !visible || event.target instanceof Element && event.target.closest('[role="dialog"],[role="menu"],[role="combobox"],[role="listbox"]')) return;
      if (event.code === "KeyE") { event.preventDefault(); void send({ interaction: { menuOpen: true }, releaseCameraInput: true }); }
      // Native-focused key handling uses this exact Rust hotbar state too.
      if (/^[1-9]$/.test(event.key)) { event.preventDefault(); void send({ interaction: { activeSlot: Number(event.key) - 1 } }); }
    };
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  }, [active, visible, menuOpen, details, send, closeMenu]);
  const forward = (kind: "down" | "up" | "move" | "wheel", event: React.MouseEvent | React.WheelEvent, fromHotbar = false) => {
    if (!fromHotbar && event.target instanceof Element && event.target.closest("[data-scene-interactive],[data-scene-ui-region],[data-native-ui-surface]")) return;
    const host = viewportRef.current;
    if (!host || !visible || menuOpen || details) return;
    // Captured clicks and relative motion already enter through the native
    // window. Do not synthesize a second down/up through WebView hit regions.
    if (snapshotRef.current?.pointerLocked && kind !== "wheel") return;
    if (kind !== "move") event.preventDefault();
    const placement = nativeClientPlacement(host);
    const scale = placement.dpiScale;
    const x = event.clientX, y = event.clientY, button = event.button;
    const delta = "deltaY" in event ? Math.max(-32768, Math.min(32767, Math.round(-event.deltaY * (event.deltaMode === 1 ? 40 : event.deltaMode === 2 ? 120 : 1)))) : undefined;
    if (kind === "wheel" && !delta) return;
    void send({ uiPointer: { kind, x: Math.round(x * scale) - placement.x, y: Math.round(y * scale) - placement.y, button, delta } });
  };
  const command = async (kind: "undo" | "redo" | "save", saveAs = false) => {
    const state = snapshotRef.current;
    if (!state || pending) return;
    setPending(true); setError("");
    try {
      let savePath: string | undefined;
      if (saveAs) {
        setSystemDialogOpen(true);
        await send({ releaseCameraInput: true });
        const selected = await saveDialog({ title: t("scene.saveAs"), defaultPath: sourcePath, filters: [{ name: "Litematic", extensions: ["litematic"] }] });
        if (!selected) return;
        savePath = selected;
      }
      await send({ interaction: { command: { kind, savePath, requestId: crypto.randomUUID(), sceneGeneration: state.sceneGeneration, expectedRevision: state.revision } } });
    } catch (reason) { setError(String(reason)); }
    finally { if (saveAs) setSystemDialogOpen(false); setPending(false); }
  };
  const selectState = (index: number, state: SceneBlockState | null) => {
    void send({ interaction: { setSlot: { index, state } } });
  };
  if (!active || !snapshot) return null;
  const fps = snapshot.cameraInputMode === "pointer_lock" || !!snapshot.pointerLocked;
  const hint = t(fps ? snapshot.mode === "edit" ? "scene.editHint" : "scene.aimHint" : snapshot.mode === "edit" ? "scene.dragEditHint" : "scene.inspectHint");
  return <>
    {systemDialogOpen && <span hidden data-scene-system-modal />}
    {toolbarTarget && createPortal(<div className="scene-editor-actions" role="group" aria-label={t("scene.edit")} title={hint}>
          <Button size="sm" variant={snapshot.mode === "observe" ? "default" : "ghost"} onClick={() => void send({ interaction: { mode: "observe" } })}>{t("scene.observe")}</Button>
          <Button size="sm" variant={snapshot.mode === "edit" ? "default" : "ghost"} onClick={() => void send({ interaction: { mode: "edit" } })}>{t("scene.edit")}</Button>
          <Button size="sm" variant="ghost" onClick={() => void send({ interaction: { menuOpen: true }, releaseCameraInput: true })}>{t("scene.inventory")}</Button>
          <DropdownMenu.Root modal={false} onOpenChange={open => {if(open)void send({releaseCameraInput:true});}}>
            <DropdownMenu.Trigger asChild><Button variant="ghost" size="icon" aria-label={t("scene.commands")} title={t("scene.commands")}><MoreHorizontal size={16}/></Button></DropdownMenu.Trigger>
            <DropdownMenu.Portal><DropdownMenu.Content className="dropdown-menu scene-edit-menu" align="end" sideOffset={8} collisionPadding={12} data-native-ui-surface>
              <DropdownMenu.Item className="dropdown-item" disabled={!snapshot.canUndo || pending} onSelect={() => void command("undo")}>{t("scene.undo")}</DropdownMenu.Item>
              <DropdownMenu.Item className="dropdown-item" disabled={!snapshot.canRedo || pending} onSelect={() => void command("redo")}>{t("scene.redo")}</DropdownMenu.Item>
              <DropdownMenu.Item className="dropdown-item" disabled={!snapshot.dirty || pending} onSelect={() => void command("save")}>{t("scene.save")}</DropdownMenu.Item>
              <DropdownMenu.Item className="dropdown-item" disabled={pending} onSelect={() => void command("save", true)}>{t("scene.saveAs")}</DropdownMenu.Item>
            </DropdownMenu.Content></DropdownMenu.Portal>
          </DropdownMenu.Root>
          <span className="scene-input-hint">{hint}</span>
        </div>, toolbarTarget)}
    {visible && !menuOpen && !details && <div className="scene-interaction-overlay" onMouseDown={event => forward("down", event)} onMouseUp={event => forward("up", event)}
      onMouseMove={event => forward("move", event)} onWheel={event => forward("wheel", event)} onContextMenu={event => event.preventDefault()}>
      {snapshot.hit && <SceneInfoHud hit={snapshot.hit} locked={!!snapshot.pointerLocked} pointer={fps ? null : snapshot.pointer} onDetails={() => { setDetails(snapshot.hit); void send({ releaseCameraInput: true }); }} />}
      <div className="scene-editor-dock" data-scene-interactive>
        <div className="scene-hotbar" role="group" aria-label={t("scene.hotbar")} onWheel={event => { event.stopPropagation(); forward("wheel", event, true); }}>
          {snapshot.hotbar.map((state, index) => <Button key={index} variant="ghost" themeSlot="hotbar-slot" data-scene-hotbar-slot={index} className="scene-hotbar-slot" aria-pressed={snapshot.activeSlot === index}
            aria-label={t("flake.hotbarItem", { index: index + 1, item: state ? translateBlockId(state.id) : t("flake.emptySlot") })}
            title={state ? translateBlockId(state.id) : t("flake.emptySlot")} onClick={() => void send({ interaction: { activeSlot: index } })}>
            {state && <BlockIcon blockId={state.id} lookupMode="item_first" />}<kbd>{index + 1}</kbd></Button>)}
        </div>


      </div>
    </div>}
    {menuOpen && <CreativeInventoryDialog onClose={closeMenu} catalog={catalog}
      quickbarSlots={snapshot.hotbar.map(state => state?.id ?? "")} typedSlots={snapshot.hotbar} onSelectState={selectState} onChangeQuickbarSlots={() => undefined} />}
    {details && <Dialog title={details.displayName || translateBlockId(details.state.id)} onClose={() => setDetails(null)} width="sm">
      <div className="scene-hud-expanded"><code>{details.state.id}</code><p>{details.position.join(", ")} · {details.distance.toFixed(1)} m</p>
        <dl>{Object.entries(details.state.properties).map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl>
        {details.container?.status === "known" && <ContainerSlots container={details.container} expanded />}
        {details.container?.status === "unknown" && <p>{details.container.error || t("scene.unknown")}</p>}
      </div></Dialog>}
  </>;
}
