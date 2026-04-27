import type { ReactNode } from "react";

export function FormRow({ label, children }: { label: string; children: ReactNode }) {
    return (
        <div className="form-row">
            <label>{label}</label>
            <div>{children}</div>
        </div>
    );
}