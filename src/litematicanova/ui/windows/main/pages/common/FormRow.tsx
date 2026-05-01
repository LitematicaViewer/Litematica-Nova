import type { ReactNode } from "react";

export function FormRow({
    label,
    children,
    htmlFor
}: {
    label: string;
    children: ReactNode;
    htmlFor?: string;
}) {
    return (
        <div className="form-row">
            <label htmlFor={htmlFor}>{label}</label>
            <div>{children}</div>
        </div>
    );
}