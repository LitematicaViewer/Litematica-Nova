// Adapted from the official shadcn/ui table registry.
"use client"

import * as React from "react"
import { cn } from "../../lib/utils"

// `forwardRef` is the one deviation from the registry: every other source-owned
// component here (button, input, checkbox, textarea) forwards a ref, and the virtual
// spacer cell needs one to publish its height. Without it a caller cannot measure the
// cell it renders.

const Table = React.forwardRef<HTMLTableElement, React.ComponentProps<"table">>(
  function Table({ className, ...props }, ref) {
    // The page already owns its scroll container; another one would break sticky
    // material headings. Keep the official table/row/cell tree directly inside it.
    return (
      <table
        ref={ref}
        data-slot="table"
        className={cn("nova-control w-full caption-bottom", className)}
        {...props}
      />
    )
  },
)

const TableHeader = React.forwardRef<HTMLTableSectionElement, React.ComponentProps<"thead">>(
  function TableHeader({ className, ...props }, ref) {
    return <thead ref={ref} data-slot="table-header" className={cn("", className)} {...props} />
  },
)

const TableBody = React.forwardRef<HTMLTableSectionElement, React.ComponentProps<"tbody">>(
  function TableBody({ className, ...props }, ref) {
    return <tbody ref={ref} data-slot="table-body" className={cn("", className)} {...props} />
  },
)

const TableFooter = React.forwardRef<HTMLTableSectionElement, React.ComponentProps<"tfoot">>(
  function TableFooter({ className, ...props }, ref) {
    return <tfoot ref={ref} data-slot="table-footer" className={cn("", className)} {...props} />
  },
)

const TableRow = React.forwardRef<HTMLTableRowElement, React.ComponentProps<"tr">>(
  function TableRow({ className, ...props }, ref) {
    return <tr ref={ref} data-slot="table-row" className={cn("transition-colors", className)} {...props} />
  },
)

const TableHead = React.forwardRef<HTMLTableCellElement, React.ComponentProps<"th">>(
  function TableHead({ className, ...props }, ref) {
    return (
      <th
        ref={ref}
        data-slot="table-head"
        className={cn(
          "h-10 px-2 text-left align-middle whitespace-nowrap [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-[2px]",
          className
        )}
        {...props}
      />
    )
  },
)

const TableCell = React.forwardRef<HTMLTableCellElement, React.ComponentProps<"td">>(
  function TableCell({ className, ...props }, ref) {
    return (
      <td
        ref={ref}
        data-slot="table-cell"
        className={cn(
          "p-2 align-middle whitespace-nowrap [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-[2px]",
          className
        )}
        {...props}
      />
    )
  },
)

const TableCaption = React.forwardRef<HTMLTableCaptionElement, React.ComponentProps<"caption">>(
  function TableCaption({ className, ...props }, ref) {
    return <caption ref={ref} data-slot="table-caption" className={cn("mt-4", className)} {...props} />
  },
)

export {
  Table,
  TableHeader,
  TableBody,
  TableFooter,
  TableHead,
  TableRow,
  TableCell,
  TableCaption,
}
