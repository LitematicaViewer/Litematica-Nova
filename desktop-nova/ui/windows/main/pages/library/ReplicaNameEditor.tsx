import { Input } from "../../../../components/ui/input";
import { ensureLitematicFileName, replicaNameError } from "./function";

/**
 * Shared file-name editor (D.11, 裁决 17).
 *
 * Used by the dispatch window's editable replica rows and reused by the trash
 * restore conflict rename and the replica reposition conflict rename (E.4 / E.7),
 * so the file-name rules live in exactly one control. `.litematic` is appended on
 * blur so the row always submits a valid projection file name.
 */
export function ReplicaNameEditor({
  value,
  onChange,
  disabled = false,
  autoFocus = false,
  ariaLabel,
  placeholder = "输入文件名",
  className = "",
}: {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  autoFocus?: boolean;
  ariaLabel?: string;
  placeholder?: string;
  className?: string;
}) {
  return (
    <Input
      className={["library-replica-name", className].filter(Boolean).join(" ")}
      value={value}
      disabled={disabled}
      autoFocus={autoFocus}
      aria-label={ariaLabel}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
      onBlur={(event) => {
        if (!event.target.value.trim()) return;
        const normalized = ensureLitematicFileName(event.target.value);
        if (normalized !== value) onChange(normalized);
      }}
    />
  );
}

/** True when the current editor value can be submitted. */
export function isReplicaNameValid(fileName: string): boolean {
  return !replicaNameError(fileName);
}