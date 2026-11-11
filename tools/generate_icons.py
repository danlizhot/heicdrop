"""Génère toutes les icônes de HeicDrop (exe, fenêtre, MSIX / Microsoft Store).

Usage :  python tools/generate_icons.py      (nécessite Pillow)
"""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BLUE = (37, 99, 235, 255)
SKY = (96, 165, 250, 255)
SUN = (251, 191, 36, 255)
WHITE = (255, 255, 255, 255)


def master(size: int = 1024, background: bool = True) -> Image.Image:
    """Dessine le logo en 1024 px puis le réduit (anti-crénelage)."""
    S = 1024
    im = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    if background:
        d.rounded_rectangle([40, 40, S - 40, S - 40], radius=200, fill=BLUE)
    card = WHITE if background else BLUE
    d.rounded_rectangle([200, 230, 824, 730], radius=60, fill=card)
    d.polygon([(250, 680), (430, 430), (560, 600), (640, 510), (774, 680)], fill=SKY)
    d.ellipse([620, 300, 720, 400], fill=SUN)
    arrow = WHITE if background else BLUE
    d.polygon([(360, 790), (664, 790), (664, 750), (760, 830), (664, 910), (664, 870), (360, 870)], fill=arrow)
    return im.resize((size, size), Image.LANCZOS) if size != S else im


def tile(w: int, h: int, logo_ratio: float = 0.66) -> Image.Image:
    """Tuile : logo centré sur fond transparent (la couleur vient du manifeste)."""
    canvas = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    side = int(min(w, h) * logo_ratio)
    logo = master(side)
    canvas.paste(logo, ((w - side) // 2, (h - side) // 2), logo)
    return canvas


def main() -> None:
    assets = ROOT / "assets"
    assets.mkdir(exist_ok=True)
    master(256).save(assets / "icon.png")
    master(1024).save(assets / "icon-1024.png")
    master(1024).save(assets / "icon.ico", sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)])

    out = ROOT / "packaging" / "msix" / "Assets"
    out.mkdir(parents=True, exist_ok=True)
    # (nom, largeur, hauteur, ratio du logo) à l'échelle 100 %
    specs = [
        ("Square44x44Logo", 44, 44, 1.0),
        ("Square71x71Logo", 71, 71, 0.75),
        ("Square150x150Logo", 150, 150, 0.6),
        ("Square310x310Logo", 310, 310, 0.55),
        ("Wide310x150Logo", 310, 150, 0.6),
        ("StoreLogo", 50, 50, 1.0),
        ("SplashScreen", 620, 300, 0.5),
    ]
    for name, w, h, r in specs:
        tile(w, h, r).save(out / f"{name}.png")  # nom de base (repli)
        for scale in (100, 125, 150, 200, 400):
            f = scale / 100
            tile(round(w * f), round(h * f), r).save(out / f"{name}.scale-{scale}.png")
    # Icônes de la barre des tâches / menu Démarrer (sans plaque)
    for t in (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256):
        img = master(t)
        img.save(out / f"Square44x44Logo.targetsize-{t}.png")
        img.save(out / f"Square44x44Logo.targetsize-{t}_altform-unplated.png")
        img.save(out / f"Square44x44Logo.targetsize-{t}_altform-lightunplated.png")

    # Visuels pour la fiche du Store
    store = ROOT / "docs" / "store" / "images"
    store.mkdir(parents=True, exist_ok=True)
    master(300).save(store / "logo-300x300.png")
    for (w, h) in [(1080, 1080), (2160, 2160)]:
        bg = Image.new("RGBA", (w, h), BLUE)
        logo = master(int(w * 0.62), background=False)
        # pastille blanche derrière le logo « sans fond »
        d = ImageDraw.Draw(bg)
        pad = int(w * 0.12)
        d.rounded_rectangle([pad, pad, w - pad, h - pad], radius=int(w * 0.12), fill=WHITE)
        bg.paste(logo, ((w - logo.width) // 2, (h - logo.height) // 2), logo)
        bg.save(store / f"box-art-{w}x{h}.png")
    print("Icônes générées.")


if __name__ == "__main__":
    main()
