#!/usr/bin/env python3
"""Package the editable Popcorn family handoff without caches or private files."""
import hashlib
import shutil
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "evidence" / "Popcorn-Family-Design-Kit.zip"


def main():
    OUT.parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="popcorn-family-") as scratch:
        kit = Path(scratch) / "Popcorn-Family-Design-Kit"
        shutil.copytree(ROOT / "design-family", kit)
        icons = kit / "icons"
        icons.mkdir()
        for name in ("PopcornStash.icon", "popcornstash.png", "popcorn.png", "popcorn.svg"):
            source = ROOT / "brand" / name
            if source.is_dir():
                shutil.copytree(source, icons / name)
            else:
                shutil.copy2(source, icons / name)
        for source in (ROOT / "brand" / "family").iterdir():
            if source.is_dir():
                shutil.copytree(source, icons / source.name)
            else:
                shutil.copy2(source, icons / source.name)
        shutil.copy2(ROOT / "web/node_modules/lucide-react/LICENSE", icons / "Lucide-LICENSE")
        fonts = kit / "fonts"
        fonts.mkdir()
        shutil.copy2(ROOT / "web/public/fonts/newsreader.woff2", fonts / "newsreader.woff2")
        shutil.copy2(ROOT / "android/app/src/main/res/font/newsreader.ttf", fonts / "newsreader-600-android.ttf")
        shutil.copy2(ROOT / "web/public/fonts/Newsreader-OFL.txt", fonts / "Newsreader-OFL.txt")
        shutil.copy2(ROOT / "LICENSE", kit / "LICENSE")
        references = kit / "references"
        references.mkdir()
        for source in (
            "DESIGN.md", "android/DESIGN.md", "web/src/themes/popcorn/popcornTheme.ts",
            "web/src/themes/popcorn/popcorn.css", "web/src/index.css",
            "android/app/src/main/kotlin/com/thekozugroup/popcornstash/ui/theme/Type.kt",
            "android/app/src/main/kotlin/com/thekozugroup/popcornstash/navigation/NavigationPlacement.kt",
            "android/app/src/main/kotlin/com/thekozugroup/popcornstash/navigation/AppShell.kt",
            "android/app/src/main/kotlin/com/thekozugroup/popcornstash/ui/components/FloatingNavToolbar.kt",
            "android/app/src/main/kotlin/com/thekozugroup/popcornstash/feature/player/FlexMode.kt",
            "android/app/src/main/kotlin/com/thekozugroup/popcornstash/feature/player/PlayerScreen.kt",
        ):
            destination = references / source
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / source, destination)
        files = sorted(p for p in kit.rglob("*") if p.is_file() and not p.name.startswith("."))
        (kit / "SHA256SUMS").write_text("".join(
            f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(kit)}\n" for p in files
        ))
        with zipfile.ZipFile(OUT, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for path in sorted(p for p in kit.rglob("*") if p.is_file() and not p.name.startswith(".")):
                archive.write(path, path.relative_to(Path(scratch)))
    print(OUT)
    print("SHA256 " + hashlib.sha256(OUT.read_bytes()).hexdigest())


if __name__ == "__main__":
    main()
