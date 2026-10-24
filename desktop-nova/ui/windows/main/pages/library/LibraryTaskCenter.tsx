import { useEffect, useState } from "react";
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

const TASK_KIND_LABEL: Record<string, string> = {
  scan: "扫描文件夹",
  checksum: "计算校验值",
  "batch-copy": "复制副本",
  "batch-rename": "批量重命名",
  "batch-trash": "移入回收区",
  restore: "还原副本",
  "export-zip": "打包导出",
  index: "构建索引",
};

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
  const [queue, setQueue] = useState<TaskQueueSnapshot>(EMPTY_QUEUE);
  const [progress, setProgress] = useState<Record<string, TaskProgressPayload>>({});
  const [finished, setFinished] = useState<TaskFinishedPayload[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    let unsubscribe: (() => void) | undefined;
    let cancelled = false;
    queryLibraryTaskQueue().then(setQueue).catch((reason) => setError(`读取任务队列失败：${String(reason)}`));
    subscribeLibraryTask({
      onProgress: (payload) => setProgress((current) => ({ ...current, [payload.taskId]: payload })),
      onQueueChanged: (snapshot) => setQueue(snapshot),
      onFinished: (payload) => {
        setFinished((current) => [payload, ...current].slice(0, 20));
        setProgress((current) => {
          const next = { ...current };
          delete next[payload.taskId];
          return next;
        });
        onFinished(payload);
      },
    }).then((handle) => { if (cancelled) handle(); else unsubscribe = handle; }).catch(() => undefined);
    return () => { cancelled = true; unsubscribe?.(); };
  }, [open]);

  const handleCancel = (taskId: string) => {
    cancelLibraryTask(taskId).catch((reason) => setError(`取消任务失败：${String(reason)}`));
  };

  if (!open) return null;
  const running = queue.running;
  const runningProgress = running ? progress[running.taskId] : undefined;

  return (
    <Dialog title="任务中心" subtitle="所有库文件操作在单一后台队列中串行执行，可随时取消。" width="md" onClose={onClose}
      footer={<Button onClick={onClose}>关闭</Button>}>
      {error && <p className="dialog-message dialog-message-error" role="alert">{error}</p>}
      <div className="library-task-center">
        <section className="library-task-section" aria-label="正在执行">
          <h4>正在执行</h4>
          {running ? (
            <div className="library-task-row" data-task-id={running.taskId}>
              <span className="library-task-title">{TASK_KIND_LABEL[running.kind] || running.kind}</span>
              {runningProgress && <span className="library-task-progress-text">{runningProgress.processed} / {runningProgress.total}</span>}
              <span className="library-task-bar" role="progressbar" aria-valuenow={runningProgress?.processed ?? 0} aria-valuemin={0} aria-valuemax={runningProgress?.total ?? 0}>
                <span className="library-task-bar-fill" style={{ width: `${runningProgress && runningProgress.total > 0 ? Math.round((runningProgress.processed / runningProgress.total) * 100) : 0}%` }} />
              </span>
              <Button size="sm" variant="ghost" onClick={() => handleCancel(running.taskId)}>取消</Button>
            </div>
          ) : <p className="muted">队列空闲。</p>}
        </section>

        <section className="library-task-section" aria-label="排队中">
          <h4>排队中 <span className="library-group-count">{queue.pending.length}</span></h4>
          {queue.pending.length ? (
            <ul className="library-task-list">
              {queue.pending.map((task) => (
                <li key={task.taskId} className="library-task-row" data-task-id={task.taskId}>
                  <span className="library-task-title">{TASK_KIND_LABEL[task.kind] || task.kind}</span>
                  <Button size="sm" variant="ghost" onClick={() => handleCancel(task.taskId)}>取消</Button>
                </li>
              ))}
            </ul>
          ) : <p className="muted">没有等待中的任务。</p>}
        </section>

        <section className="library-task-section" aria-label="最近完成">
          <h4>最近完成</h4>
          {finished.length ? (
            <ul className="library-task-list">
              {finished.map((task, index) => (
                <li key={`${task.taskId}-${index}`} className="library-task-row" data-task-status={task.status}>
                  <span className="library-task-title">{TASK_KIND_LABEL[task.kind] || task.kind}</span>
                  <span className="library-task-status">{task.status === "completed" ? "完成" : task.status === "cancelled" ? "已取消" : "失败"}{task.failures.length ? ` · ${task.failures.length} 项失败` : ""}</span>
                </li>
              ))}
            </ul>
          ) : <p className="muted">还没有完成的任务。</p>}
        </section>
      </div>
    </Dialog>
  );
}