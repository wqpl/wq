export const WQ_CAT_ART = [
  "          **********",
  "   ********        ********",
  "      ***            ****",
  "     ***              ***",
  "     ***              ***",
  "     ***              ***",
  "      ***  *  **  *  ***",
  "       ****         ***",
  "          ****  *****",
  "     **   ****          *",
  "              *********",
];

const STAR_GLYPHS = ["*", "+", "·"];
const STAR_COLOR_COUNT = 8;

export function renderWqCatConstellation() {
  let starIndex = 0;

  return WQ_CAT_ART.map((line, lineIndex) =>
    [...line]
      .map((point) => {
        if (point !== "*") return " ";

        const index = starIndex++;
        const glyph = STAR_GLYPHS[(index + lineIndex) % STAR_GLYPHS.length];
        const color = (index * 5 + lineIndex) % STAR_COLOR_COUNT;
        const delay = -(((index * 7 + lineIndex * 3) % 31) / 10);
        const duration = 2.4 + ((index * 3 + lineIndex) % 15) / 10;

        return `<span class="wq-cat-star wq-cat-star-${color}" style="--star-delay: ${delay.toFixed(1)}s; --star-duration: ${duration.toFixed(1)}s">${glyph}</span>`;
      })
      .join(""),
  ).join("\n");
}
