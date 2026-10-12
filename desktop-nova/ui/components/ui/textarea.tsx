// Adapted from the official shadcn/ui textarea registry; React 18 ref support.
import * as React from "react";
import { cn } from "../../lib/utils";
const Textarea = React.forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement>>(
  function Textarea({ className, ...props }, ref) {
    return <textarea ref={ref} data-slot="textarea" className={cn("nova-control input nova-textarea", className)} {...props} />;
  },
);
export { Textarea };
