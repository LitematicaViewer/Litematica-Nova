// Adapted from the official shadcn/ui ScrollArea registry. No business state lives here.
import * as React from "react";
import { ScrollArea as ScrollAreaPrimitive } from "radix-ui";
import { cn } from "../../lib/utils";

type ScrollAreaProps = React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.Root> & {
  horizontal?: boolean;
  viewportClassName?: string;
};
const ScrollArea = React.forwardRef<React.ElementRef<typeof ScrollAreaPrimitive.Root>, ScrollAreaProps>(
  function ScrollArea({ className, children, horizontal = false, viewportClassName, ...props }, ref) {
    return <ScrollAreaPrimitive.Root ref={ref} type="auto" data-slot="scroll-area" className={cn("nova-scroll-area", className)} {...props}>
      <ScrollAreaPrimitive.Viewport data-slot="scroll-area-viewport" className={viewportClassName} tabIndex={0}>
        {children}
      </ScrollAreaPrimitive.Viewport>
      <ScrollBar />
      {horizontal && <ScrollBar orientation="horizontal" />}
      <ScrollAreaPrimitive.Corner data-slot="scroll-area-corner" />
    </ScrollAreaPrimitive.Root>;
  },
);
function ScrollBar({ className, orientation = "vertical", ...props }: React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>) {
  return <ScrollAreaPrimitive.ScrollAreaScrollbar data-slot="scroll-area-scrollbar" orientation={orientation} className={cn("nova-scroll-bar", className)} {...props}>
    <ScrollAreaPrimitive.ScrollAreaThumb data-slot="scroll-area-thumb" />
  </ScrollAreaPrimitive.ScrollAreaScrollbar>;
}
export { ScrollArea, ScrollBar };
