import { describe, expect, it } from "vitest";
import { progressHighlightLines } from "./progress";

describe("progressHighlightLines", () => {
  it("returns empty for no ranges", () => {
    expect(progressHighlightLines([], 10)).toEqual([]);
  });

  it("expands a single range inclusively", () => {
    expect(
      progressHighlightLines([{ start_line: 2, end_line: 4 }], 10),
    ).toEqual([2, 3, 4]);
  });

  it("clamps to the document length", () => {
    expect(
      progressHighlightLines([{ start_line: 8, end_line: 15 }], 10),
    ).toEqual([8, 9, 10]);
  });

  it("drops ranges entirely past the end of the document", () => {
    expect(
      progressHighlightLines([{ start_line: 11, end_line: 20 }], 10),
    ).toEqual([]);
  });

  it("clamps start lines below 1", () => {
    expect(
      progressHighlightLines([{ start_line: 0, end_line: 2 }], 10),
    ).toEqual([1, 2]);
  });

  it("merges overlapping ranges without duplicates", () => {
    expect(
      progressHighlightLines(
        [
          { start_line: 1, end_line: 3 },
          { start_line: 2, end_line: 5 },
        ],
        10,
      ),
    ).toEqual([1, 2, 3, 4, 5]);
  });
});
