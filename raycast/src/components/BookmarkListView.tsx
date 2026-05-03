import {
  Action,
  ActionPanel,
  Alert,
  Color,
  Icon,
  List,
  Toast,
  confirmAlert,
  open,
  showToast,
} from "@raycast/api";
import { useEffect, useRef, useState } from "react";

import type { Bookmark } from "../../../shared/types";
import { deleteBookmark, listBookmarks, recordBookmarkOpen } from "../lib/api";

interface BookmarkListViewProps {
  fixedTag?: string;
  navigationTitle?: string;
}

export function BookmarkListView({
  fixedTag,
  navigationTitle = "Latch Bookmarks",
}: BookmarkListViewProps) {
  const [searchText, setSearchText] = useState("");
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [resetSelectionItemId, setResetSelectionItemId] = useState<string>();
  const [isLoading, setIsLoading] = useState(true);
  const requestIdRef = useRef(0);
  const resetSelectionTimerRef = useRef<ReturnType<typeof setTimeout>>();
  const query = searchText.trim();

  async function revalidate(options: { resetSelection?: boolean } = {}) {
    const requestId = ++requestIdRef.current;
    setIsLoading(true);

    try {
      const response = await listBookmarks({
        q: query || undefined,
        tag: fixedTag,
        limit: 100,
      });
      if (requestId !== requestIdRef.current) {
        return;
      }

      setBookmarks(response.data);
      if (options.resetSelection) {
        resetSelection(response.data[0]?.id);
      }
    } catch (error) {
      if (requestId !== requestIdRef.current) {
        return;
      }

      setBookmarks([]);
      resetSelection(undefined);
      await showToast({
        style: Toast.Style.Failure,
        title: "Failed to load bookmarks",
        message: getErrorMessage(error),
      });
    } finally {
      if (requestId === requestIdRef.current) {
        setIsLoading(false);
      }
    }
  }

  useEffect(() => {
    void revalidate({ resetSelection: true });
  }, [query, fixedTag]);

  useEffect(() => {
    return () => {
      if (resetSelectionTimerRef.current) {
        clearTimeout(resetSelectionTimerRef.current);
      }
    };
  }, []);

  function resetSelection(itemId: string | undefined) {
    if (resetSelectionTimerRef.current) {
      clearTimeout(resetSelectionTimerRef.current);
    }

    setResetSelectionItemId(itemId);

    if (!itemId) {
      return;
    }

    resetSelectionTimerRef.current = setTimeout(() => {
      setResetSelectionItemId(undefined);
    }, 50);
  }

  async function handleOpen(bookmark: Bookmark) {
    try {
      await open(bookmark.url);
    } catch (error) {
      await showToast({
        style: Toast.Style.Failure,
        title: "Failed to open link",
        message: getErrorMessage(error),
      });
      return;
    }

    void recordOpen(bookmark.id);
  }

  async function recordOpen(bookmarkId: string) {
    try {
      await recordBookmarkOpen(bookmarkId);
    } catch (error) {
      await showToast({
        style: Toast.Style.Failure,
        title: "Opened link, but failed to record the open count",
        message: getErrorMessage(error),
      });
    }
  }

  async function handleDelete(bookmark: Bookmark) {
    const confirmed = await confirmAlert({
      title: "Delete Bookmark?",
      message: `This will soft-delete "${bookmark.title || bookmark.url}".`,
      primaryAction: {
        title: "Delete",
        style: Alert.ActionStyle.Destructive,
      },
    });

    if (!confirmed) {
      return;
    }

    const toast = await showToast({
      style: Toast.Style.Animated,
      title: "Deleting bookmark",
    });

    try {
      await deleteBookmark(bookmark.id);
      toast.style = Toast.Style.Success;
      toast.title = "Bookmark deleted";
      await revalidate();
    } catch (error) {
      toast.style = Toast.Style.Failure;
      toast.title = "Delete failed";
      toast.message = getErrorMessage(error);
    }
  }

  return (
    <List
      isLoading={isLoading}
      navigationTitle={navigationTitle}
      searchBarPlaceholder={fixedTag ? `Search in ${fixedTag}` : "Search bookmarks"}
      searchText={searchText}
      filtering={false}
      onSearchTextChange={setSearchText}
      selectedItemId={resetSelectionItemId}
      throttle
    >
      {bookmarks.map((bookmark) => (
        <List.Item
          id={bookmark.id}
          key={bookmark.id}
          icon={Icon.Bookmark}
          title={bookmark.title || bookmark.url}
          subtitle={bookmark.title ? bookmark.url : undefined}
          keywords={[bookmark.url, bookmark.description, ...bookmark.tags]}
          accessories={buildAccessories(bookmark)}
          actions={
            <ActionPanel>
              <Action
                title="Open Bookmark"
                icon={Icon.Globe}
                onAction={() => handleOpen(bookmark)}
              />
              <Action.CopyToClipboard
                title="Copy Link"
                content={bookmark.url}
                shortcut={{ modifiers: ["cmd"], key: "." }}
              />
              <Action
                title="Refresh List"
                icon={Icon.ArrowClockwise}
                onAction={revalidate}
                shortcut={{ modifiers: ["cmd"], key: "r" }}
              />
              <Action
                title="Delete Bookmark"
                icon={Icon.Trash}
                style={Action.Style.Destructive}
                onAction={() => handleDelete(bookmark)}
                shortcut={{ modifiers: ["ctrl"], key: "x" }}
              />
            </ActionPanel>
          }
        />
      ))}
    </List>
  );
}

function buildAccessories(bookmark: Bookmark): List.Item.Accessory[] {
  const accessories: List.Item.Accessory[] = [];

  if (bookmark.tags.length > 0) {
    accessories.push({
      tag: {
        value: bookmark.tags.join(" · "),
        color: Color.Blue,
      },
    });
  }

  accessories.push({
    text: `Opened ${bookmark.open_count}`,
    icon: Icon.Eye,
  });

  accessories.push({
    date: new Date(bookmark.updated_at),
    tooltip: `Last updated: ${bookmark.updated_at}`,
  });

  return accessories;
}

function getErrorMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }

  return "Unknown error";
}
