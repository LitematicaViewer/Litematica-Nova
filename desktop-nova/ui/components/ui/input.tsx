// Adapted from the official shadcn/ui input registry; React 18 ref support.
import * as React from "react";
import { cn } from "../../lib/utils";

const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(function Input(
  { className, type, ...props }, ref,
) {
  return (
    <input ref={ref} type={type} data-slot="input"
      className={cn(
        "nova-control input h-9 w-full min-w-0 px-3 py-1 transition-[color,box-shadow] outline-none file:inline-flex file:h-7 disabled:pointer-events-none disabled:cursor-not-allowed",
        "",
        "", className,
      )} {...props} />
  );
});

export { Input };
