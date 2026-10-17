import assert from "node:assert/strict";
import test from "node:test";

import { relevanceBars, relevanceLabel } from "./relevance";

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

test("the label names what the bars stand for", () => {
  assert.equal(relevanceLabel({ similarity: 0.534, keywordScore: 51.63 }), "53% similar · all words found (BM25 51.6)");
  assert.equal(relevanceLabel({ similarity: 0.534, keywordScore: null }), "53% similar");
  assert.equal(relevanceLabel({ similarity: null, keywordScore: 3 }), "all words found (BM25 3.0)");
});
