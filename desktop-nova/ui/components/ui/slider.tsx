// Adapted from the official shadcn/ui slider registry.
import * as React from "react";
import { Slider as SliderPrimitive } from "radix-ui";
import { cn } from "../../lib/utils";
function Slider({ className, value, defaultValue, min = 0, max = 100, ...props }: React.ComponentPropsWithoutRef<typeof SliderPrimitive.Root>) {
  const values = value ?? defaultValue ?? [min];
  return <SliderPrimitive.Root data-slot="slider" className={cn("nova-slider", className)} value={value} defaultValue={defaultValue} min={min} max={max} {...props}>
    <SliderPrimitive.Track data-slot="slider-track"><SliderPrimitive.Range data-slot="slider-range" /></SliderPrimitive.Track>
    {values.map((_, index) => <SliderPrimitive.Thumb key={index} data-slot="slider-thumb" aria-label={props["aria-label"]} />)}
  </SliderPrimitive.Root>;
}
export { Slider };
