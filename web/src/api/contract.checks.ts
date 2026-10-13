/**
 * Compile-time checks that the client's wire types say what the server sends
 * (docs/architecture.md section 8). `npm run check` fails when one drifts. Types
 * only: nothing here reaches the bundle.
 */
import type {trash} from './client';
import type {ErrorCode, HealthResponse, ItemDetail, MediaSummary, SystemInfo} from './types';

type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
type Expect<T extends true> = T;
type Includes<Union, Member> = [Member] extends [Union] ? true : false;

export type ContractChecks = [
  // error.rs answers 413 and 405 with these codes.
  Expect<Includes<ErrorCode, 'payload_too_large'>>,
  Expect<Includes<ErrorCode, 'method_not_allowed'>>,
  // A file without a guessed type, or an ffprobe result without a format name.
  Expect<Equal<ItemDetail['mime'], string | null>>,
  Expect<Equal<MediaSummary['container'], string | null>>,
  // trash.rs: restore answers the restored item, empty answers how many it removed.
  Expect<Equal<Awaited<ReturnType<typeof trash.restore>>, ItemDetail>>,
  Expect<Equal<Awaited<ReturnType<typeof trash.empty>>, {deleted: number}>>,
  // The source code link (AGPL section 13) and STASH_HWACCEL's lock.
  Expect<Equal<HealthResponse['source'], string>>,
  Expect<Equal<SystemInfo['source'], string>>,
  Expect<Equal<SystemInfo['hwaccel']['set_by_env'], boolean>>,
];
