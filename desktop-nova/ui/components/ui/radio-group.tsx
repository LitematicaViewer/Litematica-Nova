// Adapted from the official shadcn/ui registry; see THIRD_PARTY_UI.md.
"use client"

import * as React from "react"
import { cn } from "../../lib/utils"
import { CircleIcon } from "lucide-react"
import { RadioGroup as RadioGroupPrimitive } from "radix-ui"

function RadioGroup({
  className,
  ...props
}: React.ComponentProps<typeof RadioGroupPrimitive.Root>) {
  return (
    <RadioGroupPrimitive.Root
      data-slot="radio-group"
      className={cn("nova-radio-group", className)}
      {...props}
    />
  )
}

function RadioGroupItem({
  className,
  children,
  ...props
}: React.ComponentProps<typeof RadioGroupPrimitive.Item>) {
  return (
    <RadioGroupPrimitive.Item
      data-slot="radio-group-item"
      data-choice-layout={children === undefined ? "control" : "option"}
      className={cn("nova-radio-item", className)}
      {...props}
    >
      {children ?? <RadioGroupPrimitive.Indicator
        data-slot="radio-group-indicator"
      >
        <CircleIcon aria-hidden="true" />
      </RadioGroupPrimitive.Indicator>}
    </RadioGroupPrimitive.Item>
  )
}

export { RadioGroup, RadioGroupItem }
