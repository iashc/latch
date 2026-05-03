export function parseTags(input: string) {
  return input
    .split(/[\n,，]/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

export function formatTags(tags: string[]) {
  return tags.join(", ");
}
