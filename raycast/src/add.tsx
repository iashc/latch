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
      title: "正在创建书签",
    });

    try {
      await createBookmark({
        url: values.url,
        title: values.title || undefined,
        description: values.description || undefined,
        tags: parseTags(values.tags),
      });

      toast.style = Toast.Style.Success;
      toast.title = "书签已创建";
      await popToRoot();
    } catch (error) {
      toast.style = Toast.Style.Failure;
      toast.title = "创建失败";
      toast.message = error instanceof Error ? error.message : "发生未知错误";
    }
  }

  return (
    <Form
      actions={
        <ActionPanel>
          <Action.SubmitForm title="保存书签" onSubmit={handleSubmit} />
        </ActionPanel>
      }
    >
      <Form.TextField id="url" title="URL" placeholder="https://example.com" />
      <Form.TextField id="title" title="标题" placeholder="可选" />
      <Form.TextArea id="description" title="描述" placeholder="可选" />
      <Form.TextField id="tags" title="标签" placeholder="用逗号或换行分隔" />
    </Form>
  );
}

function parseTags(input: string) {
  return input
    .split(/[\n,]/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}
