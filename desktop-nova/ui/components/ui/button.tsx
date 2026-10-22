// Adapted from the official shadcn/ui button and 8bitcn/ui button registries.
// See docs/THIRD_PARTY_UI.md. Theme changes are CSS-only; the control stays mounted.
import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "nova-control btn inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap transition-all outline-none disabled:pointer-events-none [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "",
        destructive: "",
        outline: "",
        secondary: "",
        ghost: "",
        link: "underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2 has-[>svg]:px-3",
        xs: "h-6 gap-1 px-2",
        sm: "h-8 gap-1.5 px-3",
        lg: "h-10 px-6",
        icon: "size-9",
        "icon-xs": "size-6",
        "icon-sm": "size-8",
        "icon-lg": "size-10",
      },
    },
    defaultVariants: { variant: "secondary", size: "default" },
  },
);

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
  /**
   * Overrides `data-slot` for structural containers that happen to use a button
   * element (a card tile, a navigation row). Themes style `[data-slot="button"]`
   * as an action button — 9-slice texture, fixed height, padding — and a container
   * must not inherit that. A distinct slot keeps the container on the layout side
   * of the contract without every theme having to exclude it by class.
   */
  themeSlot?: string;
}

// The stepped border and inset shadows come from the official 8bitcn button.
// They do not create a second control, and are hidden outside the pixel theme.
function ButtonDecorations({ variant, size }: Pick<ButtonProps, "variant" | "size">) {
  if (variant === "ghost" || variant === "link") return null;
  return (
    <span aria-hidden="true" className="nova-pixel-decoration pointer-events-none" data-slot="button-decorations">
      {size?.startsWith("icon") ? <>
        <span className="absolute top-0 left-0 h-[2px] w-full" />
        <span className="absolute bottom-0 left-0 h-[2px] w-full" />
        <span className="absolute top-0.5 -left-0.5 h-1/2 w-[2px]" />
        <span className="absolute bottom-0.5 -left-0.5 h-1/2 w-[2px]" />
        <span className="absolute top-0.5 -right-0.5 h-1/2 w-[2px]" />
        <span className="absolute -right-0.5 bottom-0.5 h-1/2 w-[2px]" />
      </> : <>
        <span className="absolute -top-0.5 left-0.5 h-0.5 w-1/2" />
        <span className="absolute -top-0.5 right-0.5 h-0.5 w-1/2" />
        <span className="absolute -bottom-0.5 left-0.5 h-0.5 w-1/2" />
        <span className="absolute -bottom-0.5 right-0.5 h-0.5 w-1/2" />
        <span className="absolute top-0 left-0 size-0.5" />
        <span className="absolute top-0 right-0 size-0.5" />
        <span className="absolute bottom-0 left-0 size-0.5" />
        <span className="absolute right-0 bottom-0 size-0.5" />
        <span className="absolute top-0.5 -left-0.5 h-[calc(100%-4px)] w-0.5" />
        <span className="absolute top-0.5 -right-0.5 h-[calc(100%-4px)] w-0.5" />
        {variant !== "outline" && <>
          <span className="absolute top-0 left-0 h-0.5 w-full nova-pixel-fill-soft" />
          <span className="absolute top-0.5 left-0 h-0.5 w-1 nova-pixel-fill-soft" />
          <span className="absolute bottom-0 left-0 h-0.5 w-full nova-pixel-fill-soft" />
          <span className="absolute right-0 bottom-0.5 h-0.5 w-1 nova-pixel-fill-soft" />
        </>}
      </>}
    </span>
  );
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { className, variant = "secondary", size = "default", asChild = false, themeSlot = "button", type = "button", children, ...props }, ref,
) {
  const Comp = asChild ? Slot.Root : "button";
  return (
    <Comp ref={ref} type={type} data-slot={themeSlot} data-variant={variant} data-size={size}
      className={cn(buttonVariants({ variant, size, className }))} {...props}>
      {asChild ? <Slot.Slottable>{children}</Slot.Slottable> : children}
      <span data-slot="button-motion-layer" aria-hidden="true" hidden />
      <ButtonDecorations variant={variant} size={size} />
    </Comp>
  );
});

export { Button, buttonVariants };
