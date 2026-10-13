/**
 * Calls `onReach` when a sentinel element comes within a screen of the
 * viewport. The observer is rebuilt whenever `resetKey` changes (for example
 * the number of loaded items), so a page that still does not fill the screen
 * keeps loading until it does.
 */
import {useEffect, useRef, useState} from 'react';

export function useInfiniteScroll(onReach: () => void, {enabled, resetKey}: {enabled: boolean; resetKey: unknown}) {
  const [node, setNode] = useState<Element | null>(null);
  const latest = useRef(onReach);

  useEffect(() => {
    latest.current = onReach;
  });

  useEffect(() => {
    if (!node || !enabled || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(
      entries => {
        if (entries.some(entry => entry.isIntersecting)) latest.current();
      },
      {rootMargin: '900px 0px'},
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [node, enabled, resetKey]);

  return setNode;
}
