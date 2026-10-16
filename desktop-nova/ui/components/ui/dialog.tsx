// Adapted from the official shadcn/ui dialog and 8bitcn/ui dialog registries.
import * as React from "react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { cn } from "../../lib/utils";

const Dialog = DialogPrimitive.Root;
const DialogTrigger = DialogPrimitive.Trigger;
const DialogClose = DialogPrimitive.Close;
const DialogPortal = DialogPrimitive.Portal;

const DialogOverlay = React.forwardRef<React.ElementRef<typeof DialogPrimitive.Overlay>, React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>>(
  function DialogOverlay({ className, ...props }, ref) {
    return <DialogPrimitive.Overlay ref={ref} data-slot="dialog-overlay"
      className={cn("dialog-overlay fixed inset-0", className)} {...props} />;
  },
);

const DialogContent = React.forwardRef<React.ElementRef<typeof DialogPrimitive.Content>, React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content>>(
  function DialogContent({ className, children, ...props }, ref) {
    return (
      <DialogPortal>
        <DialogOverlay>
          <DialogPrimitive.Content ref={ref} data-slot="dialog-content" asChild
            className={cn("nova-control dialog-surface outline-none", className)} {...props}>
            <section>
              {children}
              <div aria-hidden="true" className="nova-pixel-decoration pointer-events-none absolute inset-0 -mx-0.5" data-slot="dialog-pixel-x" />
              <div aria-hidden="true" className="nova-pixel-decoration pointer-events-none absolute inset-0 -my-0.5" data-slot="dialog-pixel-y" />
            </section>
          </DialogPrimitive.Content>
        </DialogOverlay>
      </DialogPortal>
    );
  },
);

const DialogTitle = React.forwardRef<React.ElementRef<typeof DialogPrimitive.Title>, React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>>(
  function DialogTitle({ className, ...props }, ref) {
    return <DialogPrimitive.Title ref={ref} data-slot="dialog-title" className={cn("dialog-title", className)} {...props} />;
  },
);
const DialogDescription = React.forwardRef<React.ElementRef<typeof DialogPrimitive.Description>, React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>>(
  function DialogDescription({ className, ...props }, ref) {
    return <DialogPrimitive.Description ref={ref} data-slot="dialog-description" className={cn("dialog-subtitle muted", className)} {...props} />;
  },
);

export { Dialog, DialogClose, DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogTitle, DialogTrigger };
