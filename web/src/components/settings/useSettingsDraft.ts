/**
 * Loads the owner settings (`GET /api/settings`), keeps an editable draft of
 * the fields a section shows, and saves only what changed with a partial
 * `PUT /api/settings` (the server merges).
 */
import {useCallback, useState} from 'react';
import {settings as settingsApi} from '../../api/client';
import type {Settings, SettingsPatch} from '../../api/types';
import {useResource} from './useResource';

export interface SettingsDraft<T> {
  saved: Settings | undefined;
  draft: T | null;
  error: unknown;
  isLoading: boolean;
  isDirty: boolean;
  reload: () => void;
  edit: (update: (current: T) => T) => void;
  reset: () => void;
  /** Saves the patch and adopts what the server answers. */
  save: (patch: SettingsPatch) => Promise<Settings>;
}

export function useSettingsDraft<T>(
  pick: (settings: Settings) => T,
  equals: (a: T, b: T) => boolean = (a, b) => JSON.stringify(a) === JSON.stringify(b),
): SettingsDraft<T> {
  const resource = useResource(signal => settingsApi.get(signal), {key: 'settings'});
  const [edits, setEdits] = useState<{base: Settings; value: T} | null>(null);
  const saved = resource.data;

  // Edits only count against the settings they were made on.
  const baseline = saved ? pick(saved) : null;
  const draft = edits && saved && edits.base === saved ? edits.value : baseline;
  const isDirty = Boolean(draft && baseline && !equals(draft, baseline));

  const edit = useCallback(
    (update: (current: T) => T) => {
      if (!saved) return;
      setEdits(current => {
        const start = current && current.base === saved ? current.value : pick(saved);
        return {base: saved, value: update(start)};
      });
    },
    [saved, pick],
  );

  const reset = useCallback(() => setEdits(null), []);

  const {mutate} = resource;
  const save = useCallback(
    async (patch: SettingsPatch) => {
      const next = await settingsApi.update(patch);
      mutate(() => next);
      setEdits(null);
      return next;
    },
    [mutate],
  );

  return {
    saved,
    draft,
    error: resource.error,
    isLoading: resource.isLoading,
    isDirty,
    reload: resource.reload,
    edit,
    reset,
    save,
  };
}
