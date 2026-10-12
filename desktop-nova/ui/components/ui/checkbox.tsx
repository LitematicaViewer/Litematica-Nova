// Adapted from shadcn/ui and 8bitcn/ui official registries; see THIRD_PARTY_UI.md.
import * as React from "react";
import { Checkbox as CheckboxPrimitive } from "radix-ui";
import { Check, Minus } from "lucide-react";
import { cn } from "../../lib/utils";

const Checkbox = React.forwardRef<React.ElementRef<typeof CheckboxPrimitive.Root>, React.ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>>(
  function Checkbox({ className, ...props }, ref) {
    return <CheckboxPrimitive.Root ref={ref} data-slot="checkbox"
      className={cn("nova-checkbox", className)} {...props}>
      <CheckboxPrimitive.Indicator data-slot="checkbox-indicator">
        {/* The primitive's state is authoritative for controlled AND defaultChecked use. */}
        <Check data-slot="checkbox-check" aria-hidden="true" />
        <Minus data-slot="checkbox-mixed" aria-hidden="true" />
      </CheckboxPrimitive.Indicator>
      <span className="nova-checkbox-pixel-edge" aria-hidden="true" />
    </CheckboxPrimitive.Root>;
  },
);
export { Checkbox };
