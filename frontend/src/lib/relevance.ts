/** Similarity at which a hit gets all bars. Near the top of what Qwen3-Embedding gives a close
 * match, so the bars spread over the range real hits fall in instead of all looking alike. */
const SIMILARITY_FOR_ALL_BARS = 0.75;

export const RELEVANCE_BARS = 4;

/**
 * How many of the four bars a hit fills: its similarity between the model's cut-off and
 * `SIMILARITY_FOR_ALL_BARS`, or for a keyword-only hit its BM25 score against the best one shown.
 * Every hit fills at least one: it passed the cut-off or matched every word.
 */
export function relevanceBars(
  hit: { similarity?: number | null; keywordScore?: number | null },
  { minSimilarity, bestKeywordScore }: { minSimilarity: number; bestKeywordScore: number },
): number {
  const share =
    hit.similarity != null
      ? (hit.similarity - minSimilarity) / (SIMILARITY_FOR_ALL_BARS - minSimilarity)
      : hit.keywordScore != null && bestKeywordScore > 0
        ? hit.keywordScore / bestKeywordScore
        : 0;
  return 1 + Math.round(Math.min(Math.max(share, 0), 1) * (RELEVANCE_BARS - 1));
}
