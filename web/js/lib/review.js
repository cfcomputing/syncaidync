/** Pure review mutations — always return new objects. */

function clone(review) {
  return {
    reviewed: { ...(review?.reviewed || {}) },
    notes: [...(review?.notes || [])],
    overall: review?.overall ?? "",
    plan: review?.plan ?? null,
    updated_at: review?.updated_at ?? 0,
  };
}

export function isReviewed(review, file) {
  if (!review || !file?.path) return false;
  const dig = review.reviewed?.[file.path];
  return !!dig && dig === file.digest;
}

export function progress(review, files) {
  const list = files || [];
  let reviewed = 0;
  for (const f of list) {
    if (isReviewed(review, f)) reviewed += 1;
  }
  return { reviewed, total: list.length };
}

export function toggleReviewed(review, file) {
  const next = clone(review);
  if (!file?.path) return next;
  if (isReviewed(next, file)) {
    delete next.reviewed[file.path];
  } else if (file.digest) {
    next.reviewed[file.path] = file.digest;
  }
  return next;
}

export function addNote(review, note) {
  const next = clone(review);
  next.notes = [...next.notes, { ...note }];
  return next;
}

export function editNote(review, id, patch) {
  const next = clone(review);
  next.notes = next.notes.map((n) => (n.id === id ? { ...n, ...patch, id: n.id } : n));
  return next;
}

export function removeNote(review, id) {
  const next = clone(review);
  next.notes = next.notes.filter((n) => n.id !== id);
  return next;
}

export function notesFor(review, path) {
  return (review?.notes || []).filter((n) => n.path === path);
}

export function unsent(review) {
  return (review?.notes || []).filter((n) => !n.sent);
}

export function markSent(review, ids) {
  const set = new Set(ids || []);
  const next = clone(review);
  next.notes = next.notes.map((n) => (set.has(n.id) ? { ...n, sent: true } : n));
  return next;
}
