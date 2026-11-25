import React, { useState, useEffect, useRef, useMemo, useCallback } from 'react';

export interface VirtualGridProps<T> {
  items: T[];
  itemKey: (item: T, index: number) => string | number;
  renderItem: (item: T, index: number) => React.ReactNode;
  minItemWidth?: number;
  rowHeight?: number;
  gap?: number;
  containerHeight?: number | string;
  maxHeight?: number | string;
  className?: string;
  style?: React.CSSProperties;
  emptyContent?: React.ReactNode;
  targetVisibleCount?: number;
}

export function VirtualGrid<T>({
  items,
  itemKey,
  renderItem,
  minItemWidth = 320,
  rowHeight = 200,
  gap = 12,
  containerHeight = 540,
  maxHeight,
  className,
  style,
  emptyContent,
  targetVisibleCount = 30,
}: VirtualGridProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [measuredWidth, setMeasuredWidth] = useState(0);
  const [measuredHeight, setMeasuredHeight] = useState(0);

  // ResizeObserver to detect container dimensions
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const updateDimensions = () => {
      if (el.clientWidth > 0) setMeasuredWidth(el.clientWidth);
      if (el.clientHeight > 0) setMeasuredHeight(el.clientHeight);
    };

    updateDimensions();

    if (typeof ResizeObserver !== 'undefined') {
      const observer = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const { width, height } = entry.contentRect;
          if (width > 0) setMeasuredWidth(width);
          if (height > 0) setMeasuredHeight(height);
        }
      });
      observer.observe(el);
      return () => observer.disconnect();
    } else {
      window.addEventListener('resize', updateDimensions);
      return () => window.removeEventListener('resize', updateDimensions);
    }
  }, []);

  // In headless/JSDOM environments clientWidth/clientHeight can be 0.
  // We use robust desktop defaults (960px width, 540px height) so testing always renders.
  const effectiveWidth = measuredWidth > 0 ? measuredWidth : 960;
  const effectiveHeight =
    measuredHeight > 0
      ? measuredHeight
      : typeof containerHeight === 'number'
        ? containerHeight
        : 540;

  // Number of columns based on container width
  const columns = useMemo(() => {
    return Math.max(1, Math.floor((effectiveWidth + gap) / (minItemWidth + gap)));
  }, [effectiveWidth, minItemWidth, gap]);

  const totalRows = useMemo(() => {
    return Math.ceil(items.length / columns);
  }, [items.length, columns]);

  const totalHeight = useMemo(() => {
    if (totalRows === 0) return 0;
    return totalRows * (rowHeight + gap) - gap;
  }, [totalRows, rowHeight, gap]);

  // Handle scroll events with RAF throttling for high-fps performance
  const rafId = useRef<number | null>(null);
  const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
    const nextScrollTop = e.currentTarget.scrollTop;
    if (rafId.current !== null) {
      cancelAnimationFrame(rafId.current);
    }
    rafId.current = requestAnimationFrame(() => {
      setScrollTop(nextScrollTop);
      rafId.current = null;
    });
  }, []);

  useEffect(() => {
    return () => {
      if (rafId.current !== null) {
        cancelAnimationFrame(rafId.current);
      }
    };
  }, []);

  // Calculate visible range, keeping rendered cards stable around targetVisibleCount (~30)
  const { startRow, endRow } = useMemo(() => {
    if (totalRows === 0) return { startRow: 0, endRow: 0 };

    const rowTotal = rowHeight + gap;
    const visibleInViewport = Math.max(1, Math.ceil(effectiveHeight / rowTotal));
    // Determine target rows to satisfy targetVisibleCount (e.g. 30 items)
    const targetRows = Math.max(visibleInViewport, Math.ceil(targetVisibleCount / columns));
    const overscan = Math.max(1, Math.floor((targetRows - visibleInViewport) / 2));

    const currentStart = Math.floor(scrollTop / rowTotal);
    const start = Math.max(0, currentStart - overscan);
    const end = Math.min(totalRows, currentStart + visibleInViewport + overscan);

    return { startRow: start, endRow: end };
  }, [totalRows, rowHeight, gap, effectiveHeight, targetVisibleCount, columns, scrollTop]);

  // Slice visible items organized by rows
  const visibleRowSlices = useMemo(() => {
    const rows: { rowIndex: number; top: number; items: { item: T; index: number }[] }[] = [];
    for (let r = startRow; r < endRow; r++) {
      const rowItems: { item: T; index: number }[] = [];
      const rowStartIdx = r * columns;
      for (let c = 0; c < columns; c++) {
        const itemIdx = rowStartIdx + c;
        if (itemIdx < items.length) {
          rowItems.push({ item: items[itemIdx], index: itemIdx });
        }
      }
      rows.push({
        rowIndex: r,
        top: r * (rowHeight + gap),
        items: rowItems,
      });
    }
    return rows;
  }, [startRow, endRow, columns, items, rowHeight, gap]);

  if (items.length === 0 && emptyContent) {
    return <>{emptyContent}</>;
  }

  return (
    <div
      ref={containerRef}
      onScroll={handleScroll}
      className={className}
      style={{
        position: 'relative',
        overflowY: 'auto',
        overflowX: 'hidden',
        height: containerHeight,
        maxHeight,
        width: '100%',
        willChange: 'transform',
        ...style,
      }}
      role="feed"
      aria-label="virtual-grid-container"
    >
      <div
        style={{
          height: totalHeight,
          width: '100%',
          position: 'relative',
          pointerEvents: 'auto',
        }}
      >
        {visibleRowSlices.map(({ rowIndex, top, items: rowItems }) => (
          <div
            key={rowIndex}
            style={{
              position: 'absolute',
              top,
              left: 0,
              right: 0,
              height: rowHeight,
              display: 'grid',
              gridTemplateColumns: `repeat(${columns}, 1fr)`,
              gap,
              alignItems: 'stretch',
            }}
          >
            {rowItems.map(({ item, index }) => (
              <React.Fragment key={itemKey(item, index)}>{renderItem(item, index)}</React.Fragment>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

export default VirtualGrid;
