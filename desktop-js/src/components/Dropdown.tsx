import React, { useState, useRef, useEffect } from "react";

export function Dropdown({ value, options, onChange, renderValue }: { value: string, options: {label: string, value: string}[], onChange: (v: string) => void, renderValue?: (v: string) => string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const handleClick = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, []);
  
  const display = renderValue ? renderValue(value) : (options.find(o => o.value === value)?.label || value);
  return (
    <div className="dropdown-container" ref={ref}>
      <button type="button" className="btn" onClick={() => setOpen(!open)}>{display} ▾</button>
      {open && (
        <div className="dropdown-menu">
          {options.map(o => (
            <div key={o.value} className="dropdown-item" onClick={() => { onChange(o.value); setOpen(false); }}>
              {o.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
