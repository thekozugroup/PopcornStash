/**
 * Window-wide file drop tracking for the "drop anywhere" overlay.
 *
 * Reports while files are dragged over the window and hands dropped files to
 * `onFiles`. Dropped folders are opened and every file inside is included
 * (hidden files such as .DS_Store are skipped). The app shell routes the
 * files to Downloads (`navigate('/downloads', {state: {droppedFiles}})`),
 * where the Downloads page starts the resumable uploads.
 */
import {useEffect, useRef, useState} from 'react';

/** Safety limit for one drop of nested folders. */
const MAX_DROPPED_FILES = 5000;
/**
 * Browsers fire `dragover` at least every ~550ms while a drag hovers over the
 * page. If none arrives for longer, the drag left the window without a
 * matching `dragleave` (it happens across iframes and some window managers).
 */
const DRAG_WATCHDOG_MS = 1200;

function hasFiles(event: DragEvent): boolean {
  return Array.from(event.dataTransfer?.types ?? []).includes('Files');
}

function isHidden(name: string): boolean {
  return name.startsWith('.') || name.startsWith('~$');
}

function readEntries(reader: FileSystemDirectoryReader): Promise<FileSystemEntry[]> {
  return new Promise((resolve, reject) => reader.readEntries(resolve, reject));
}

function entryFile(entry: FileSystemFileEntry): Promise<File> {
  return new Promise((resolve, reject) => entry.file(resolve, reject));
}

async function collect(entry: FileSystemEntry, out: File[]): Promise<void> {
  if (out.length >= MAX_DROPPED_FILES || isHidden(entry.name)) return;
  if (entry.isFile) {
    try {
      out.push(await entryFile(entry as FileSystemFileEntry));
    } catch {
      // Unreadable file (permissions, removed meanwhile): skip it.
    }
    return;
  }
  if (entry.isDirectory) {
    const reader = (entry as FileSystemDirectoryEntry).createReader();
    // readEntries returns batches; an empty batch means the folder is done.
    for (;;) {
      let batch: FileSystemEntry[];
      try {
        batch = await readEntries(reader);
      } catch {
        return;
      }
      if (batch.length === 0) return;
      for (const child of batch) await collect(child, out);
      if (out.length >= MAX_DROPPED_FILES) return;
    }
  }
}

/** Files from a drop, opening folders. Entries must be taken during the drop event itself. */
function droppedFiles(dataTransfer: DataTransfer): Promise<File[]> | File[] {
  const items = Array.from(dataTransfer.items ?? []).filter(item => item.kind === 'file');
  const entries = items.map(item => item.webkitGetAsEntry?.() ?? null);
  const plain = Array.from(dataTransfer.files ?? []);
  if (!entries.some(entry => entry?.isDirectory)) {
    return plain.filter(file => !isHidden(file.name));
  }
  return (async () => {
    const out: File[] = [];
    for (const [index, entry] of entries.entries()) {
      if (entry) {
        await collect(entry, out);
      } else {
        const file = items[index]?.getAsFile();
        if (file && !isHidden(file.name)) out.push(file);
      }
    }
    return out;
  })();
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
    if (!enabled) return;
    let watchdog: ReturnType<typeof setTimeout> | null = null;

    const stop = () => {
      depth.current = 0;
      if (watchdog) clearTimeout(watchdog);
      watchdog = null;
      setIsDragging(false);
    };
    const keepAlive = () => {
      if (watchdog) clearTimeout(watchdog);
      watchdog = setTimeout(stop, DRAG_WATCHDOG_MS);
    };

    const onDragEnter = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      depth.current += 1;
      setIsDragging(true);
      keepAlive();
    };
    const onDragOver = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
      setIsDragging(true);
      keepAlive();
    };
    const onDragLeave = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) stop();
    };
    const onDrop = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      stop();
      if (!event.dataTransfer) return;
      const result = droppedFiles(event.dataTransfer);
      if (Array.isArray(result)) {
        if (result.length > 0) latest.current(result);
        return;
      }
      void result.then(files => {
        if (files.length > 0) latest.current(files);
      });
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') stop();
    };

    window.addEventListener('dragenter', onDragEnter);
    window.addEventListener('dragover', onDragOver);
    window.addEventListener('dragleave', onDragLeave);
    window.addEventListener('drop', onDrop);
    window.addEventListener('dragend', stop);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      stop();
      window.removeEventListener('dragenter', onDragEnter);
      window.removeEventListener('dragover', onDragOver);
      window.removeEventListener('dragleave', onDragLeave);
      window.removeEventListener('drop', onDrop);
      window.removeEventListener('dragend', stop);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [enabled]);

  return {isDragging: enabled && isDragging};
}
