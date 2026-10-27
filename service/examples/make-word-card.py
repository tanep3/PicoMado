#!/usr/bin/env python3
"""Make a 160x80 LCD card from a phrase using an installed Japanese font."""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('words')
    parser.add_argument('--font', required=True, type=Path)
    parser.add_argument('--size', type=int, default=24)
    parser.add_argument('--x', type=int, default=8)
    parser.add_argument('--y', type=int, default=22)
    parser.add_argument('--output', type=Path, default=Path('service/examples/word-card.png'))
    args = parser.parse_args()
    if not 8 <= args.size <= 48 or not args.font.is_file():
        parser.error('choose an installed font and size 8..48')
    font = ImageFont.truetype(str(args.font), args.size, index=0)
    card = Image.new('RGB', (160, 80), '#102030')
    draw = ImageDraw.Draw(card)
    box = draw.textbbox((args.x, args.y), args.words, font=font, anchor='lt')
    if box[0] < 0 or box[1] < 0 or box[2] > 160 or box[3] > 80:
        parser.error('text does not fit 160x80; shorten it or change size/position')
    draw.text((args.x, args.y), args.words, font=font, fill='#ffffff', anchor='lt')
    card.save(args.output)
    print(args.output)


if __name__ == '__main__':
    main()
