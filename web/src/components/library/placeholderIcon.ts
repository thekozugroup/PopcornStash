/**
 * The icon a tile shows when a file has no picture: music, documents and
 * archives get their own, everything else keeps the tile's default (film,
 * clapperboard or photo).
 */
import {FileArchive, FileText, Music, type LucideIcon} from 'lucide-react';
import type {Item} from '../../api/types';
import {fileType} from './libraryFormat';

const ARCHIVES = new Set(['ZIP', 'RAR', '7Z', 'TAR', 'GZ', 'ISO']);

export function placeholderIcon(item: Pick<Item, 'kind' | 'rel_path'>): LucideIcon | undefined {
  if (item.kind === 'audio') return Music;
  if (ARCHIVES.has(fileType(item.rel_path) ?? '')) return FileArchive;
  if (item.kind === 'other') return FileText;
  return undefined;
}
