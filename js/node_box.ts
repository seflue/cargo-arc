// node_box.ts - Where the collapse symbol and the level number stand in the
// box of a row, and how wide a collapsed row has to be. The numbers match
// LAYOUT in src/render/constants.rs.

/** The collapse symbol starts this far before the right edge of the box. */
const TOGGLE_OFFSET = 14;
/** The gap between the level number and the collapse symbol. */
const LEVEL_GAP = 6;
/** The width of one digit of a level number. */
const LEVEL_CHAR_WIDTH = 6;
/** The room around a label inside its box. */
const BOX_PADDING = 20;

const NodeBox = {
  TOGGLE_OFFSET,

  /**
   * The x at which the right-aligned level number of a row ends.
   * @param nodeX - Left edge of the box
   * @param width - Width of the box
   */
  levelEndX(nodeX: number, width: number): number {
    return nodeX + width - TOGGLE_OFFSET - LEVEL_GAP;
  },

  /**
   * The width a collapsed row needs for its label, `labelWidth` wide.
   * A row with a level also keeps room for the number and the collapse
   * symbol, so that neither touches the label.
   * @param levelText - The level number as shown, empty for a row without one
   */
  collapsedWidth(labelWidth: number, levelText: string): number {
    const width = labelWidth + BOX_PADDING;
    if (!levelText) return width;
    return (
      width + TOGGLE_OFFSET + levelText.length * LEVEL_CHAR_WIDTH + LEVEL_GAP
    );
  },

  /**
   * The label of a row without the level number that ends its text.
   * @param text - The text content of the label element
   */
  labelText(text: string, levelText: string): string {
    return levelText && text.endsWith(levelText)
      ? text.slice(0, -levelText.length)
      : text;
  },
};

export { NodeBox };
