import React, { useRef } from "react";
import { X } from "lucide-react";
import { Button } from "./ui/button";
import { ScrollArea } from "./ui/scroll-area";
import { Dialog as DialogRoot, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";

export interface DialogProps {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
  width?: "sm" | "md" | "lg";
  className?: string;
  busy?: boolean;
}

export function Dialog({ title, subtitle, onClose, children, footer, width = "md", className = "", busy = false }: DialogProps) {
  // Page state opens these dialogs without a Radix Trigger. Preserve that opener;
  // Radix owns the focus trap, nested dialogs and keyboard/outside dismissal.
  const previousFocus = useRef(typeof document !== "undefined" ? document.activeElement : null);
  const surfaceClassName = [`dialog-surface-${width}`, className].filter(Boolean).join(" ");
  return (
    <DialogRoot open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
      <DialogContent className={surfaceClassName} aria-busy={busy} {...(subtitle ? {} : { "aria-describedby": undefined })}
        onEscapeKeyDown={(event) => { if (busy) event.preventDefault(); }}
        onInteractOutside={(event) => { if (busy) event.preventDefault(); }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (previousFocus.current instanceof HTMLElement && previousFocus.current.isConnected) previousFocus.current.focus();
        }}>
        <header className="dialog-header">
          <div className="dialog-title-stack">
            <DialogTitle asChild><h3>{title}</h3></DialogTitle>
            {subtitle ? <DialogDescription>{subtitle}</DialogDescription> : null}
          </div>
          <Button className="dialog-close-button" size="icon" aria-label="关闭对话框" onClick={onClose} disabled={busy}><X size={18} aria-hidden="true" /></Button>
        </header>
        <ScrollArea className="dialog-body">{children}</ScrollArea>
        {footer ? <footer className="dialog-footer">{footer}</footer> : null}
      </DialogContent>
    </DialogRoot>
  );
}
