import {
  Action,
  ActionPanel,
  Form,
  Toast,
  popToRoot,
  showToast,
} from "@raycast/api";

import { createBookmark } from "./lib/api";

interface Values {
  url: string;
  title: string;
  description: string;
  tags: string;
}

export default function AddBookmarkCommand() {
  async function handleSubmit(values: Values) {
    const toast = await showToast({
      style: Toast.Style.Animated,
      title: "Creating bookmark",
    });

    try {
      await createBookmark({
        url: values.url,
        title: values.title || undefined,
        description: values.description || undefined,
        tags: parseTags(values.tags),
      });

      toast.style = Toast.Style.Success;
      toast.title = "Bookmark created";
      await popToRoot();
    } catch (error) {
      toast.style = Toast.Style.Failure;
      toast.title = "Create failed";
      toast.message = error instanceof Error ? error.message : "Unknown error";
    }
  }

  return (
    <Form
      actions={
        <ActionPanel>
          <Action.SubmitForm title="Save Bookmark" onSubmit={handleSubmit} />
        </ActionPanel>
      }
    >
      <Form.TextField id="url" title="URL" placeholder="https://example.com" />
      <Form.TextField id="title" title="Title" placeholder="Optional" />
      <Form.TextArea id="description" title="Description" placeholder="Optional" />
      <Form.TextField id="tags" title="Tags" placeholder="Separate with commas or new lines" />
    </Form>
  );
}

function parseTags(input: string) {
  return input
    .split(/[\n,]/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}
