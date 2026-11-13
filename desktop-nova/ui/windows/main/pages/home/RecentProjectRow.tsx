import React, { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { FileText } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { ThemeIcon } from "../../../../components/ThemeIcon";

export interface RecentProjectRowProps {
  name: string;
  date: string;
  previewUrl?: string;
  title?: string;
  busy?: boolean;
  disabled?: boolean;
  onOpen?: () => void;
}

/**
 * Recent-project row. The thumbnail is never shown inline: every row keeps the
 * same file icon, and a selected-style card carrying the 140px preview follows
 * the pointer (or the focused row) while it is hovered.
 *
 * Positioning follows the local-library browser preview: render a fixed popup,
 * measure it, and flip/clamp it against the viewport instead of assuming a size.
 */
export function RecentProjectRow({
  name,
  date,
  previewUrl,
  title,
  busy = false,
  disabled = false,
  onOpen,
}: RecentProjectRowProps) {
  const [hovered, setHovered] = useState(false);
  const [pointer, setPointer] = useState({ x: 0, y: 0 });
  const [popupPosition, setPopupPosition] = useState({ left: 0, top: 0 });
  const popupRef = useRef<HTMLDivElement | null>(null);
  const showPreview = hovered && !disabled && !!previewUrl;
  const isLitematic = /\.litematic$/i.test(name) || /\.litematic$/i.test(title ?? "");

  useLayoutEffect(() => {
    if (!showPreview) return;
    const popup = popupRef.current;
    const offset = 14;
    const margin = 4;
    if (!popup) {
      setPopupPosition({ left: pointer.x + offset, top: pointer.y + offset });
      return;
    }

    const rect = popup.getBoundingClientRect();
    let left = pointer.x + offset;
    let top = pointer.y + offset;

    if (left + rect.width > window.innerWidth - margin) {
      left = pointer.x - rect.width - offset;
    }
    if (top + rect.height > window.innerHeight - margin) {
      top = pointer.y - rect.height - offset;
    }

    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin));
    setPopupPosition((previous) => previous.left === left && previous.top === top ? previous : { left, top });
  }, [pointer.x, pointer.y, showPreview]);

  useLayoutEffect(() => {
    const popup = popupRef.current;
    if (!popup) return;
    popup.style.left = `${popupPosition.left}px`;
    popup.style.top = `${popupPosition.top}px`;
  }, [popupPosition.left, popupPosition.top]);

  return (
    <>
      <Button
        variant="ghost"
        type="button"
        title={title}
        aria-busy={busy}
        disabled={disabled}
        onMouseEnter={(event) => {
          if (!previewUrl) return;
          setPointer({ x: event.clientX, y: event.clientY });
          setHovered(true);
        }}
        onMouseMove={(event) => {
          if (!previewUrl) return;
          setPointer({ x: event.clientX, y: event.clientY });
        }}
        onMouseLeave={() => setHovered(false)}
        onFocus={(event) => {
          if (!previewUrl) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          setPointer({ x: bounds.left, y: bounds.top });
          setHovered(true);
        }}
        onBlur={() => setHovered(false)}
        onClick={onOpen}
      >
        <ThemeIcon set="file" name={isLitematic ? "litematic" : "unknown"} size={16} fallback={FileText} />
        <span>{name}</span>
        <small>{date}</small>
      </Button>
      {showPreview && previewUrl ? createPortal(
        <div ref={popupRef} className="home-recent-preview-popup" role="tooltip">
          <div className="nova-card home-recent-preview nova-float-card" data-slot="card" data-variant="selected">
            <div className="home-recent-preview-box">
              <img src={previewUrl} alt="" width={140} height={140} />
            </div>
          </div>
        </div>,
        document.body,
      ) : null}
    </>
  );
}
