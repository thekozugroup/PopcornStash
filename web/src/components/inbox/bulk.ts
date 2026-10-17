import type {ItemDetail, Library} from '../../api/types';
import {needsDetails, resolveTarget, targetChoices, targetValue, type TargetValue} from './places';

export type InboxFileType = 'video' | 'photo' | 'subtitle' | 'audio' | 'other';
export function inboxFileType(item: Pick<ItemDetail, 'kind' | 'rel_path'>): InboxFileType {
  const extension = item.rel_path.split('.').pop()?.toLowerCase() ?? '';
  if (['srt', 'ass', 'ssa', 'sub', 'idx', 'vtt', 'sup', 'smi', 'ttml'].includes(extension)) return 'subtitle';
  return item.kind;
}
export function matchingInbox(items: ItemDetail[], download: string, type: string): ItemDetail[] {
  return items.filter(item => (download === 'all' || (download === 'none' ? !item.source_download_id : item.source_download_id === download)) && (type === 'all' || inboxFileType(item) === type));
}
/** Revalidate permissions and required episode details for every selected item. */
export function bulkReady(items: ItemDetail[], libraries: Library[], owner: boolean, targetOf: (item: ItemDetail) => TargetValue | null) {
  return items.flatMap(item => {
    const value = targetOf(item);
    const target = resolveTarget(value, libraries);
    return value && target && targetChoices(item, libraries, owner).some(choice => targetValue(choice) === value) && !needsDetails(target, item)
      ? [{item, value}] : [];
  });
}
