import assert from "node:assert/strict";
import test from "node:test";

import { relevanceBars } from "./relevance";

const context = { minSimilarity: 0.42, bestKeywordScore: 50 };

test("similarity fills the bars from the cut-off up", () => {
  assert.equal(relevanceBars({ similarity: 0.42 }, context), 1);
  assert.equal(relevanceBars({ similarity: 0.53 }, context), 2);
  assert.equal(relevanceBars({ similarity: 0.64 }, context), 3);
  assert.equal(relevanceBars({ similarity: 0.75 }, context), 4);
  assert.equal(relevanceBars({ similarity: 0.9 }, context), 4);
});

test("a keyword-only hit gets its bars from its BM25 against the best one shown", () => {
  assert.equal(relevanceBars({ similarity: null, keywordScore: 50 }, context), 4);
  assert.equal(relevanceBars({ similarity: null, keywordScore: 5 }, context), 1);
  assert.equal(relevanceBars({ similarity: null, keywordScore: 51.6 }, { ...context, bestKeywordScore: 0 }), 1);
});
