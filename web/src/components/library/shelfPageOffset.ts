interface ShelfGeometry {
  scrollLeft: number;
  contentWidth: number;
  cardWidth: number;
  stride: number;
}

/** Page by the number of complete cards that fit between the shelf gutters. */
export function shelfPageOffset({scrollLeft, contentWidth, cardWidth, stride}: ShelfGeometry, direction: 1 | -1) {
  // In an RTL grid successive cards extend left and scrollLeft is negative.
  const step = Math.abs(stride);
  const sign = Math.sign(stride);
  const position = scrollLeft * sign;
  const gap = step - cardWidth;
  const cardsPerPage = Math.max(1, Math.floor((contentWidth + gap + 1) / step));
  // A swipe or the last page can stop between snap points. Count the partial
  // leading card when going forward, and the partial trailing card going back.
  // Allow one CSS pixel for fractional positions and clientWidth rounding.
  const leading = direction === 1
    ? Math.floor((position + 1) / step)
    : Math.ceil((position - 1) / step);
  return sign * Math.max(0, (leading + direction * cardsPerPage) * step);
}
