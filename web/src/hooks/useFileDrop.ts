/**
 * Window-wide file drop tracking for the "drop anywhere" overlay.
 *
 * Stub contract for the upload flow: the hook only reports when files are
 * dragged over the window and hands dropped files to `onFiles`. The app shell
 * routes them to Downloads (`navigate('/downloads', {state: {droppedFiles}})`),
 * where the Downloads page starts the resumable uploads (`src/api/upload.ts`).
 */
import {useEffect, useRef, useState} from 'react';

function hasFiles(event: DragEvent): boolean {
  return Array.from(event.dataTransfer?.types ?? []).includes('Files');
}

export interface FileDropOptions {
  onFiles: (files: File[]) => void;
  enabled?: boolean;
}

export function useFileDrop({onFiles, enabled = true}: FileDropOptions): {isDragging: boolean} {
  const [isDragging, setIsDragging] = useState(false);
  const depth = useRef(0);
  const latest = useRef(onFiles);

  useEffect(() => {
    latest.current = onFiles;
  });

  useEffect(() => {
    if (!enabled) {
      depth.current = 0;
      setIsDragging(false);
      return;
    }

    const onDragEnter = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      depth.current += 1;
      setIsDragging(true);
    };
    const onDragOver = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
    };
    const onDragLeave = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) setIsDragging(false);
    };
    const onDrop = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      depth.current = 0;
      setIsDragging(false);
      const files = Array.from(event.dataTransfer?.files ?? []);
      if (files.length > 0) latest.current(files);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        depth.current = 0;
        setIsDragging(false);
      }
    };

    window.addEventListener('dragenter', onDragEnter);
    window.addEventListener('dragover', onDragOver);
    window.addEventListener('dragleave', onDragLeave);
    window.addEventListener('drop', onDrop);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('dragenter', onDragEnter);
      window.removeEventListener('dragover', onDragOver);
      window.removeEventListener('dragleave', onDragLeave);
      window.removeEventListener('drop', onDrop);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [enabled]);

  return {isDragging};
}
