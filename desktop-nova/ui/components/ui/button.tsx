// Adapted from the official shadcn/ui button and 8bitcn/ui button registries.
// See docs/THIRD_PARTY_UI.md. Theme changes are CSS-only; the control stays mounted.
import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "nova-control btn inline-flex shrink-0 items-center justify-center gap-2 rounded-md text-sm font-medium whitespace-nowrap transition-all outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-destructive/20 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary/90",
        destructive: "bg-destructive text-white hover:bg-destructive/90 focus-visible:ring-destructive/20",
        outline: "border bg-background shadow-xs hover:bg-accent hover:text-accent-foreground",
        secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        ghost: "hover:bg-accent hover:text-accent-foreground",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2 has-[>svg]:px-3",
        xs: "h-6 gap-1 rounded-md px-2 text-xs",
        sm: "h-8 gap-1.5 rounded-md px-3",
        lg: "h-10 rounded-md px-6",
        icon: "size-9",
        "icon-xs": "size-6 rounded-md",
        "icon-sm": "size-8",
        "icon-lg": "size-10",
      },
    },
    defaultVariants: { variant: "secondary", size: "default" },
  },
);

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

// The stepped border and inset shadows come from the official 8bitcn button.
// They do not create a second control, and are hidden outside the pixel theme.
function ButtonDecorations({ variant, size }: Pick<ButtonProps, "variant" | "size">) {
  if (variant === "ghost" || variant === "link") return null;
  return (
    <span aria-hidden="true" className="nova-pixel-decoration pointer-events-none" data-slot="button-decorations">
      {size?.startsWith("icon") ? <>
        <span className="absolute top-0 left-0 h-[2px] w-full bg-foreground" />
        <span className="absolute bottom-0 left-0 h-[2px] w-full bg-foreground" />
        <span className="absolute top-0.5 -left-0.5 h-1/2 w-[2px] bg-foreground" />
        <span className="absolute bottom-0.5 -left-0.5 h-1/2 w-[2px] bg-foreground" />
        <span className="absolute top-0.5 -right-0.5 h-1/2 w-[2px] bg-foreground" />
        <span className="absolute -right-0.5 bottom-0.5 h-1/2 w-[2px] bg-foreground" />
      </> : <>
        <span className="absolute -top-0.5 left-0.5 h-0.5 w-1/2 bg-foreground" />
        <span className="absolute -top-0.5 right-0.5 h-0.5 w-1/2 bg-foreground" />
        <span className="absolute -bottom-0.5 left-0.5 h-0.5 w-1/2 bg-foreground" />
        <span className="absolute -bottom-0.5 right-0.5 h-0.5 w-1/2 bg-foreground" />
        <span className="absolute top-0 left-0 size-0.5 bg-foreground" />
        <span className="absolute top-0 right-0 size-0.5 bg-foreground" />
        <span className="absolute bottom-0 left-0 size-0.5 bg-foreground" />
        <span className="absolute right-0 bottom-0 size-0.5 bg-foreground" />
        <span className="absolute top-0.5 -left-0.5 h-[calc(100%-4px)] w-0.5 bg-foreground" />
        <span className="absolute top-0.5 -right-0.5 h-[calc(100%-4px)] w-0.5 bg-foreground" />
        {variant !== "outline" && <>
          <span className="absolute top-0 left-0 h-0.5 w-full bg-foreground/20" />
          <span className="absolute top-0.5 left-0 h-0.5 w-1 bg-foreground/20" />
          <span className="absolute bottom-0 left-0 h-0.5 w-full bg-foreground/20" />
          <span className="absolute right-0 bottom-0.5 h-0.5 w-1 bg-foreground/20" />
        </>}
      </>}
    </span>
  );
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { className, variant = "secondary", size = "default", asChild = false, type = "button", children, ...props }, ref,
) {
  const Comp = asChild ? Slot.Root : "button";
  return (
    <Comp ref={ref} type={type} data-slot="button" data-variant={variant} data-size={size}
      className={cn(buttonVariants({ variant, size, className }))} {...props}>
      {asChild ? <Slot.Slottable>{children}</Slot.Slottable> : children}
      <ButtonDecorations variant={variant} size={size} />
    </Comp>
  );
});

export { Button, buttonVariants };
