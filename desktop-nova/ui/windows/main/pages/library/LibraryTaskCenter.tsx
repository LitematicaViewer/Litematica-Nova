import { t } from "../../../../../src/services/i18n";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../../../components/ui/button";
import { Dialog } from "../../../../components/Dialog";
import {
  cancelLibraryTask,
  queryLibraryTaskQueue,
  subscribeLibraryTask,
  type TaskFinishedPayload,
  type TaskProgressPayload,
  type TaskQueueSnapshot,
} from "../../../../../src/services/libraryTasks";



const EMPTY_QUEUE: TaskQueueSnapshot = { pending: [], running: null };

/**
 * Task centre (D.10, spec 4.4.3): observes the single serial queue through the three
 * `projection-library-task-*` events and lets the user cancel a pending/running task.
 * Cancellation is cooperative: the terminal event still reports the partial result.
 */
export function LibraryTaskCenter({
  open,
  onClose,
  onFinished,
}: {
  open: boolean;
  onClose: () => void;
  onFinished: (payload: TaskFinishedPayload) => void;
}) {
  const TASK_KIND_LABEL: Record<string, string> = {
    scan: t("library.ux.scanFolders"),
    checksum: t("library.ux.checksum"),
    "batch-copy": t("library.ux.copyReplicas"),
    "batch-rename": t("library.ux.renameBatch"),
    "batch-trash": t("library.ux.trash"),
    restore: t("library.ux.restoreReplicas"),
    "export-zip": t("library.ux.exportZip"),
    index: t("library.ux.buildIndex"),
  };
  const [queue, setQueue] = useState<TaskQueueSnapshot>(EMPTY_QUEUE);
  const [progress, setProgress] = useState<Record<string, TaskProgressPayload>>({});
  const [finished, setFinished] = useState<TaskFinishedPayload[]>([]);
  const [error, setError] = useState("");
  const finishedCallback = useRef(onFinished);
  finishedCallback.current = onFinished;

  useEffect(() => {
    if (!open) return;
    let unsubscribe: (() => void) | undefined;
    let cancelled = false;
    let receivedQueue = false;
    setError("");
    subscribeLibraryTask({
      onProgress: (payload) => { if (!cancelled) setProgress((current) => ({ ...current, [payload.taskId]: payload })); },
      onQueueChanged: (snapshot) => { if (!cancelled) { receivedQueue = true; setQueue(snapshot); } },
      onFinished: (payload) => {
        if (cancelled) return;
        setFinished((current) => [payload, ...current].slice(0, 20));
        setProgress((current) => {
          const next = { ...current };
          delete next[payload.taskId];
          return next;
        });
        finishedCallback.current(payload);
      },
    }).then(async (handle) => {
      if (cancelled) { handle(); return; }
      unsubscribe = handle;
      const snapshot = await queryLibraryTaskQueue();
      if (!cancelled && !receivedQueue) setQueue(snapshot);
    }).catch(reason => { if (!cancelled) setError(t("library.ux.queueReadFailed", { value0: String(reason) })); });
    return () => { cancelled = true; unsubscribe?.(); };
  }, [open]);

  const handleCancel = (taskId: string) => {
    cancelLibraryTask(taskId).catch((reason) => setError(t("library.ux.queueCancelFailed", { value0: String(reason) })));
  };

  if (!open) return null;
  const running = queue.running;
  const runningProgress = running ? progress[running.taskId] : undefined;

  return (
    <Dialog title={t("library.ux.taskCenter")} subtitle={t("library.ux.queueHint")} width="md" onClose={onClose}
      footer={<Button onClick={onClose}>{t("common.close")}</Button>}>
      {error && <p className="dialog-message dialog-message-error" role="alert">{error}</p>}
      <div className="library-task-center">
        <section className="library-task-section" aria-label={t("library.ux.running")}>
          <h4>{t("library.ux.running")}</h4>
          {running ? (
            <div className="library-task-row" data-task-id={running.taskId}>
              <span className="library-task-title">{TASK_KIND_LABEL[running.kind] || running.kind}</span>
              {runningProgress && <span className="library-task-progress-text">{runningProgress.processed} / {runningProgress.total}</span>}
              <span className="library-task-bar" role="progressbar" aria-label={TASK_KIND_LABEL[running.kind] || running.kind} aria-valuenow={runningProgress && runningProgress.total > 0 ? runningProgress.processed : undefined} aria-valuemin={0} aria-valuemax={runningProgress && runningProgress.total > 0 ? runningProgress.total : undefined}>
                <span className="library-task-bar-fill" style={{ width: `${runningProgress && runningProgress.total > 0 ? Math.round((runningProgress.processed / runningProgress.total) * 100) : 0}%` }} />
              </span>
              <Button size="sm" variant="ghost" onClick={() => handleCancel(running.taskId)}>{t("common.cancel")}</Button>
            </div>
          ) : <p className="muted">{t("library.ux.queueIdle")}</p>}
        </section>

        <section className="library-task-section" aria-label={t("library.ux.queued")}>
          <h4>{t("library.ux.queued")} <span className="library-group-count">{queue.pending.length}</span></h4>
          {queue.pending.length ? (
            <ul className="library-task-list">
              {queue.pending.map((task) => (
                <li key={task.taskId} className="library-task-row" data-task-id={task.taskId}>
                  <span className="library-task-title">{TASK_KIND_LABEL[task.kind] || task.kind}</span>
                  <Button size="sm" variant="ghost" onClick={() => handleCancel(task.taskId)}>{t("common.cancel")}</Button>
                </li>
              ))}
            </ul>
          ) : <p className="muted">{t("library.ux.noQueued")}</p>}
        </section>

        <section className="library-task-section" aria-label={t("library.ux.recentCompleted")}>
          <h4>{t("library.ux.recentCompleted")}</h4>
          {finished.length ? (
            <ul className="library-task-list">
              {finished.map((task, index) => (
                <li key={`${task.taskId}-${index}`} className="library-task-row" data-task-status={task.status}>
                  <span className="library-task-title">{TASK_KIND_LABEL[task.kind] || task.kind}</span>
                  <span className="library-task-status">{task.status === "completed" ? t("ui.done") : task.status === "cancelled" ? t("library.ux.cancelled") : t("generate.status.failed")}{task.failures.length ? t("library.ux.failures", { value0: task.failures.length }) : ""}</span>
                </li>
              ))}
            </ul>
          ) : <p className="muted">{t("library.ux.noCompleted")}</p>}
        </section>
      </div>
    </Dialog>
  );
}
