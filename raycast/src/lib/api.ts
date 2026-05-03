import { getPreferenceValues } from "@raycast/api";

import type {
  Bookmark,
  BookmarkListResponse,
  CreateBookmarkRequest,
  ErrorPayload,
  ListBookmarksParams,
  TagListResponse,
} from "../../../shared/types";

const DEFAULT_SERVER_URL = "http://127.0.0.1:52525";

interface Preferences {
  serverUrl?: string;
}

export class LatchApiError extends Error {
  constructor(
    message: string,
    public readonly code?: string,
    public readonly status?: number,
    public readonly details?: Record<string, unknown>,
  ) {
    super(message);
    this.name = "LatchApiError";
  }
}

function getBaseUrl() {
  const preferences = getPreferenceValues<Preferences>();
  return (preferences.serverUrl?.trim() || DEFAULT_SERVER_URL).replace(/\/+$/, "");
}

function buildUrl(path: string, params?: Record<string, string | number | undefined>) {
  const url = new URL(path, `${getBaseUrl()}/`);
  for (const [key, value] of Object.entries(params ?? {})) {
    if (value === undefined || value === "") {
      continue;
    }
    url.searchParams.set(key, String(value));
  }
  return url;
}

async function readResponse<T>(response: Response): Promise<T> {
  const raw = await response.text();
  const parsed = raw ? JSON.parse(raw) : null;

  if (!response.ok) {
    const errorPayload = parsed as ErrorPayload | null;
    throw new LatchApiError(
      errorPayload?.error.message || `Request failed with status ${response.status}`,
      errorPayload?.error.code,
      response.status,
      errorPayload?.error.details,
    );
  }

  return parsed as T;
}

async function request<T>(path: string, init?: RequestInit) {
  const response = await fetch(buildUrl(path), {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
  });

  return readResponse<T>(response);
}

export async function listBookmarks(params: ListBookmarksParams = {}) {
  return request<BookmarkListResponse>(
    buildUrl("/api/bookmarks", {
      q: params.q,
      tag: params.tag,
      url: params.url,
      since: params.since,
      until: params.until,
      offset: params.offset,
      limit: params.limit ?? 100,
    }).toString(),
  );
}

export async function createBookmark(payload: CreateBookmarkRequest) {
  return request<Bookmark>("/api/bookmarks", {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export async function deleteBookmark(id: string) {
  return request<Bookmark>(`/api/bookmarks/${id}`, {
    method: "DELETE",
  });
}

export async function recordBookmarkOpen(id: string) {
  return request<Bookmark>(`/api/bookmarks/${id}/open`, {
    method: "POST",
  });
}

export async function listTags() {
  return request<TagListResponse>("/api/bookmarks/tags");
}
