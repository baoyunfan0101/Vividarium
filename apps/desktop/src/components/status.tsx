import type { ReactNode } from "react";
export function LoadingOverlay({ label }: { label: string }) {
  return (
    <div className="loading-overlay" aria-live="polite">
      <div className="spinner" />
      <span>{label}</span>
    </div>
  );
}

export function AdminActionArea({
  active,
  label,
  processed,
  total,
  children
}: {
  active: boolean;
  label: string;
  processed?: number;
  total?: number | null;
  children: ReactNode;
}) {
  if (active) {
    const percent = total ? Math.min(((processed ?? 0) / total) * 100, 100) : null;
    return (
      <div className="admin-progress">
        <div className={percent === null ? "progress-bar" : "progress-bar determinate"}>
          <span style={percent === null ? undefined : { width: `${percent}%` }} />
        </div>
        <strong>{label}{total ? ` ${processed ?? 0}/${total}` : ""}</strong>
      </div>
    );
  }
  return <>{children}</>;
}
