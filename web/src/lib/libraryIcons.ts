/**
 * The curated library icons (docs/architecture.md section 8, Icons): each
 * Lucide name the server accepts, its lucide-react glyph and its plain name
 * for tooltips and screen readers. The sidebar, the phone menu, the quick
 * links, Home, tile placeholders, empty states and the icon picker all draw a
 * library's icon through `libraryIcon()`, so they never disagree.
 *
 * The order here is the picker's fallback order; the server's
 * `GET /api/libraries/icons` order wins when it has loaded. A name this build
 * does not know (a newer server) draws `folder`, which is not in the list:
 * it also marks folders that are not in a library, so it never stands for an
 * icon the owner chose. The plain names are the Android app's too
 * (`LibraryIcons.kt`); its unit tests compare the two tables.
 */
import {
  Aperture,
  Baby,
  Bike,
  BookOpen,
  Cake,
  Camera,
  Clapperboard,
  Drama,
  Film,
  Folder,
  Gamepad2,
  Gift,
  GraduationCap,
  Heart,
  ImageIcon,
  Images,
  MountainSnow,
  Music,
  PartyPopper,
  PawPrint,
  Plane,
  Popcorn,
  Projector,
  Snowflake,
  Star,
  Sun,
  Tent,
  TreePalm,
  Trophy,
  Tv,
  Users,
  Video,
  type LucideIcon,
  type LucideProps,
} from 'lucide-react';
import {createElement} from 'react';
import type {LibraryIcon} from '../api/types';

interface IconEntry {
  icon: LucideIcon;
  /** Sentence case, for the tooltip and the accessible name. */
  label: string;
}

const ICONS: Record<string, IconEntry> = {
  film: {icon: Film, label: 'Film'},
  clapperboard: {icon: Clapperboard, label: 'Clapperboard'},
  tv: {icon: Tv, label: 'TV'},
  popcorn: {icon: Popcorn, label: 'Popcorn'},
  projector: {icon: Projector, label: 'Projector'},
  video: {icon: Video, label: 'Video camera'},
  drama: {icon: Drama, label: 'Theatre masks'},
  image: {icon: ImageIcon, label: 'Picture'},
  images: {icon: Images, label: 'Pictures'},
  camera: {icon: Camera, label: 'Camera'},
  aperture: {icon: Aperture, label: 'Aperture'},
  users: {icon: Users, label: 'People'},
  baby: {icon: Baby, label: 'Baby'},
  heart: {icon: Heart, label: 'Heart'},
  cake: {icon: Cake, label: 'Cake'},
  'party-popper': {icon: PartyPopper, label: 'Party popper'},
  'graduation-cap': {icon: GraduationCap, label: 'Graduation cap'},
  gift: {icon: Gift, label: 'Gift'},
  plane: {icon: Plane, label: 'Plane'},
  'tree-palm': {icon: TreePalm, label: 'Palm tree'},
  'mountain-snow': {icon: MountainSnow, label: 'Mountain'},
  tent: {icon: Tent, label: 'Tent'},
  sun: {icon: Sun, label: 'Sun'},
  snowflake: {icon: Snowflake, label: 'Snowflake'},
  'gamepad-2': {icon: Gamepad2, label: 'Game controller'},
  music: {icon: Music, label: 'Music'},
  'book-open': {icon: BookOpen, label: 'Book'},
  trophy: {icon: Trophy, label: 'Trophy'},
  'paw-print': {icon: PawPrint, label: 'Paw print'},
  bike: {icon: Bike, label: 'Bike'},
  star: {icon: Star, label: 'Star'},
};

/** Every name this build can draw, in picker order. */
export const KNOWN_LIBRARY_ICONS: LibraryIcon[] = Object.keys(ICONS);

/** The glyph for a library's icon name; `folder` for a name this build does not know. */
export function libraryIcon(name: LibraryIcon | null | undefined): LucideIcon {
  return (name ? ICONS[name]?.icon : undefined) ?? Folder;
}

/** The icon's plain name ("Party popper"). */
export function libraryIconLabel(name: LibraryIcon): string {
  return ICONS[name]?.label ?? name.replace(/-/g, ' ').replace(/^\w/, letter => letter.toUpperCase());
}

/** A library's glyph, hidden from screen readers (its name is always beside it). */
export function LibraryGlyph({name, size = 18, strokeWidth = 1.5, ...rest}: {name: LibraryIcon | null | undefined} & LucideProps) {
  return createElement(libraryIcon(name), {size, strokeWidth, 'aria-hidden': true, ...rest});
}

/** What a new library starts on until the owner picks. */
export function defaultIconFor(contentType: 'video' | 'photos'): LibraryIcon {
  return contentType === 'photos' ? 'image' : 'film';
}
