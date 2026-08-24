import { useEffect, useRef, useState, type RefObject } from "react";

/**
 * Detects whether the header toolbar content overflows the available width
 * and returns a `compact` flag for the AppSwitcher.
 *
 * The container ref must point at a flex-constrained element
 * (`flex-1 min-w-0 overflow-hidden`) whose width is determined by the parent
 * layout, not its own content — avoiding the oscillation problem when
 * toggling compact mode.
 *
 * The content ref points at the inner flex row. Its children may include a
 * horizontally scrollable region (the app switcher strip), whose own
 * scrollWidth is clamped by layout, so the desired width is measured by
 * summing each child's scrollWidth instead of reading the row's scrollWidth.
 */
export function useAutoCompact(
  containerRef: RefObject<HTMLDivElement | null>,
  contentRef: RefObject<HTMLDivElement | null>,
): boolean {
  const [compact, setCompact] = useState(false);
  const normalWidthRef = useRef(0);
  const lockUntilRef = useRef(0);

  useEffect(() => {
    const el = containerRef.current;
    const content = contentRef.current;
    if (!el || !content) return;

    const measureDesiredWidth = () => {
      const children = Array.from(content.children) as HTMLElement[];
      if (children.length === 0) return content.scrollWidth;
      const gap = parseFloat(getComputedStyle(content).columnGap) || 0;
      return (
        children.reduce((sum, child) => sum + child.scrollWidth, 0) +
        gap * (children.length - 1)
      );
    };

    const ro = new ResizeObserver(() => {
      // During expand animation, ignore resize events to prevent flicker
      if (Date.now() < lockUntilRef.current) return;

      if (!compact) {
        // Overflow detected → switch to compact
        const desired = measureDesiredWidth();
        if (desired > el.clientWidth + 1) {
          // Cache only at the overflow edge: when content fits,
          // desired === clientWidth, so caching unconditionally
          // would pollute normalWidthRef with the container width (e.g. after
          // maximizing), making the expand threshold unreachable.
          normalWidthRef.current = desired;
          setCompact(true);
        }
      } else if (normalWidthRef.current > 0) {
        // In compact mode: only recover to normal if
        // available space >= what normal mode needed
        if (el.clientWidth >= normalWidthRef.current) {
          // Lock out resize events during the expand animation (200ms + 50ms margin)
          lockUntilRef.current = Date.now() + 250;
          setCompact(false);
        }
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [compact, containerRef, contentRef]);

  return compact;
}
