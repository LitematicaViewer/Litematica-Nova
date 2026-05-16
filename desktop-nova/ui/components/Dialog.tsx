import React from "react";

export interface DialogProps {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
  width?: "sm" | "md" | "lg";
  className?: string;
}

export function Dialog({
  title,
  subtitle,
  onClose,
  children,
  footer,
  width = "md",
  className = "",
}: DialogProps) {
  const surfaceClassName = [
    "dialog-surface",
    `dialog-surface-${width}`,
    className,
  ].filter(Boolean).join(" ");

  return (
    <div className="dialog-overlay" role="presentation" onClick={onClose}>
      <section
        className={surfaceClassName}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="dialog-header">
          <div className="dialog-title-stack">
            <h3 className="dialog-title">{title}</h3>
            {subtitle ? <p className="dialog-subtitle muted">{subtitle}</p> : null}
          </div>
          <button className="btn dialog-close-button" type="button" aria-label="关闭对话框" onClick={onClose}>×</button>
        </header>
        <div className="dialog-body">{children}</div>
        {footer ? <footer className="dialog-footer">{footer}</footer> : null}
      </section>
    </div>
  );
}