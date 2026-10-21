export type ThreadIdentity = { source: string; thread: string };
export type ThreadFacet = ThreadIdentity & { title: string | null; count: number };

/** A JSON pair object keeps slashes and arbitrary origin thread ids unambiguous in URLs. */
export function threadKey({ source, thread }: ThreadIdentity): string {
  return JSON.stringify({ source, thread });
}

export function parseThread(value: string): ThreadIdentity {
  const parsed: unknown = JSON.parse(value);
  if (
    parsed === null || typeof parsed !== "object" || Array.isArray(parsed) ||
    !("source" in parsed) || typeof parsed.source !== "string" ||
    !("thread" in parsed) || typeof parsed.thread !== "string" ||
    Object.keys(parsed).some((key) => key !== "source" && key !== "thread")
  ) {
    throw new Error("A thread filter must contain source and thread strings");
  }
  return { source: parsed.source, thread: parsed.thread };
}

export function toggleThread(threads: ThreadIdentity[], thread: ThreadIdentity): ThreadIdentity[] {
  const key = threadKey(thread);
  return threads.some((value) => threadKey(value) === key)
    ? threads.filter((value) => threadKey(value) !== key)
    : [...threads, { source: thread.source, thread: thread.thread }];
}

export function withSelectedThreads(facets: ThreadFacet[], selected: ThreadIdentity[]): ThreadFacet[] {
  const present = new Set(facets.map(threadKey));
  return [
    ...selected.filter((thread) => !present.has(threadKey(thread))).map((thread) => ({ ...thread, title: null, count: 0 })),
    ...facets,
  ];
}

export function threadLabel(thread: ThreadFacet, facets: ThreadFacet[]): string {
  const title = thread.title || thread.thread;
  const duplicates = facets.filter((other) => (other.title || other.thread) === title);
  if (duplicates.length < 2) return title;
  return duplicates.some((other) => other.source === thread.source && other.thread !== thread.thread)
    ? `${title} · ${thread.source} · ${thread.thread}`
    : `${title} · ${thread.source}`;
}
