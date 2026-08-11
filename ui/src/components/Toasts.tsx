import { useEffect } from "preact/hooks";

export interface Toast {
  id: number;
  message: string;
}

/** Transient notifications, bottom right; each dismisses itself. */
export function Toasts({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}) {
  return (
    <div class="toasts" role="status">
      {toasts.map((toast) => (
        <ToastCard key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
    </div>
  );
}

function ToastCard({
  toast,
  onDismiss,
}: {
  toast: Toast;
  onDismiss: (id: number) => void;
}) {
  useEffect(() => {
    const timer = setTimeout(() => onDismiss(toast.id), 8000);
    return () => clearTimeout(timer);
  }, [toast.id]);
  return (
    <div class="toast">
      <span>{toast.message}</span>
      <button aria-label="dismiss" onClick={() => onDismiss(toast.id)}>
        ✕
      </button>
    </div>
  );
}
