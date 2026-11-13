import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import styled from "styled-components";

import type { ServerSettings } from "../api/bamboo";
import { PanelTitle, PixelButton, PixelInput, PixelPanel, PixelSlider } from "../components/pixel";
import {
  describeDefault,
  FIELDS,
  fieldError,
  GROUPS,
  type NumberField,
  SECTIONS,
  type SettingField,
} from "../settings/fields";
import { useServer } from "../state/ServerContext";

const Form = styled.form`
  display: flex;
  flex-direction: column;
  gap: 12px;
`;

const Grid = styled.div`
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px 16px;
  max-width: 720px;
`;

const Field = styled.label<{ $wide?: boolean }>`
  display: flex;
  flex-direction: column;
  gap: 4px;
  grid-column: ${({ $wide }) => ($wide ? "1 / -1" : "auto")};
`;

const Row = styled.div`
  display: flex;
  align-items: center;
  gap: 8px;
`;

const Hint = styled.span`
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

/** 页面和分组的说明文字。 */
const Lead = styled.p`
  margin: 0 0 12px;
  color: ${({ theme }) => theme.color.panelTextMuted};
  line-height: 1.6;
`;

const ErrorText = styled.span`
  color: ${({ theme }) => theme.color.redstone.base};
`;

/** 服务器运行中时的提醒，用红石色和面板区分开。 */
const Notice = styled.p`
  margin: 8px 0 0;
  padding: 6px 10px;
  border-left: 4px solid ${({ theme }) => theme.color.redstone.base};
  background: rgba(0, 0, 0, 0.08);
`;

const Actions = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
`;

/** 输入框里显示的数字。NaN 表示正在编辑或者填了非数字，显示成空。 */
const numberText = (value: number) => (Number.isNaN(value) ? "" : String(value));

export function SettingsPage() {
  const { settings, defaults, snapshot, updateSettings } = useServer();
  const [draft, setDraft] = useState<ServerSettings | null>(settings);
  /** 数字输入框正在编辑的原文，只在当前这一轮编辑里盖住草稿的数值。 */
  const [editing, setEditing] = useState<Partial<Record<keyof ServerSettings, string>>>({});
  const [revealSecret, setRevealSecret] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setDraft(settings);
    setEditing({});
  }, [settings]);

  if (!draft || !defaults) {
    return null;
  }

  const errors = new Map<keyof ServerSettings, string>();
  for (const field of FIELDS) {
    const error = fieldError(field, draft);
    if (error) {
      errors.set(field.key, error);
    }
  }
  const changed = JSON.stringify(draft) !== JSON.stringify(settings);

  const setValue = <K extends keyof ServerSettings>(key: K, value: ServerSettings[K]) => {
    setDraft({ ...draft, [key]: value });
    setMessage(null);
  };

  /** 数字输入框。原文留着，这样 `-` 这种半成品不会被马上吞掉。 */
  const setNumber = (field: NumberField, text: string) => {
    setEditing({ ...editing, [field.key]: text });
    setValue(field.key, text.trim().length === 0 ? Number.NaN : Number(text));
  };

  const restoreDefaults = () => {
    setDraft(defaults);
    setEditing({});
    setMessage("已填入默认值，还没有保存");
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await updateSettings(draft);
      setMessage(snapshot?.running ? "已保存，重启服务器后生效" : "已保存");
    } catch (reason) {
      setMessage(String(reason));
    }
  };

  const control = (field: SettingField): ReactNode => {
    const invalid = errors.has(field.key);
    const disabled = field.disabled?.(draft) ?? false;

    switch (field.control) {
      case "text":
      case "password":
        return (
          <Row>
            <PixelInput
              type={field.control === "password" && !revealSecret ? "password" : "text"}
              aria-label={field.label}
              value={draft[field.key]}
              maxLength={field.maxLength}
              disabled={disabled}
              aria-invalid={invalid}
              spellCheck={false}
              onChange={(event) => setValue(field.key, event.target.value)}
            />
            {field.control === "password" && (
              <PixelButton
                type="button"
                $size="small"
                aria-pressed={revealSecret}
                onClick={() => setRevealSecret(!revealSecret)}
              >
                {revealSecret ? "隐藏" : "显示"}
              </PixelButton>
            )}
          </Row>
        );
      case "number":
        return (
          <Row>
            <PixelInput
              inputMode="numeric"
              aria-label={field.label}
              value={editing[field.key] ?? numberText(draft[field.key])}
              disabled={disabled}
              aria-invalid={invalid}
              onChange={(event) => setNumber(field, event.target.value)}
            />
            {field.unit && <Hint>{field.unit}</Hint>}
          </Row>
        );
      case "slider":
        return (
          <PixelSlider
            label={field.label}
            min={field.min}
            max={field.max}
            // 手改过设置文件时值可能越界，滑块本身不显示越界位置，错误提示会说明。
            value={Math.min(field.max, Math.max(field.min, draft[field.key] || field.min))}
            onChange={(value) => setValue(field.key, value)}
          />
        );
      case "choice":
        return (
          <Row role="radiogroup" aria-label={field.label}>
            {field.options.map((option) => (
              <PixelButton
                key={option.value}
                type="button"
                $size="small"
                $variant={draft[field.key] === option.value ? "bamboo" : "stone"}
                role="radio"
                aria-checked={draft[field.key] === option.value}
                onClick={() => setValue(field.key, option.value)}
              >
                {option.label}
              </PixelButton>
            ))}
          </Row>
        );
      case "toggle":
        return (
          <Row>
            <PixelButton
              type="button"
              $size="small"
              $variant={draft[field.key] ? "bamboo" : "stone"}
              role="switch"
              aria-label={field.label}
              aria-checked={draft[field.key]}
              aria-invalid={invalid}
              disabled={disabled}
              onClick={() => setValue(field.key, !draft[field.key])}
            >
              {draft[field.key] ? "已打开" : "已关闭"}
            </PixelButton>
          </Row>
        );
    }
  };

  return (
    <>
      <PixelPanel>
        <PanelTitle>服务器设置</PanelTitle>
        <Lead>
          下面每一项都是服务端启动时读一次，保存之后要重启服务器才会生效。默认值写在每一项下面。
          官方设置写进 server.properties，Bamboo 优化设置写进 bamboo.toml。
        </Lead>
        <Lead>服务器目录：{draft.serverDirectory || settings?.serverDirectory}</Lead>
        <Lead>
          命令行版用 <code>bamboo --server-dir {draft.serverDirectory || "这个目录"}</code>{" "}
          就能读到完全相同的两份文件。
        </Lead>
        {snapshot?.running && (
          <Notice role="status">服务器正在运行，现在保存的修改要到下次启动时才生效。</Notice>
        )}
      </PixelPanel>

      <Form onSubmit={(event) => void handleSubmit(event)}>
        {SECTIONS.map((section) => (
          <div key={section.id}>
            <PixelPanel>
              <PanelTitle>{section.title}</PanelTitle>
              <Lead>{section.hint}</Lead>
            </PixelPanel>
            {GROUPS.filter((group) => group.section === section.id).map((group) => (
              <PixelPanel key={group.id}>
                <PanelTitle>{group.title}</PanelTitle>
                <Lead>{group.hint}</Lead>
                <Grid>
                  {FIELDS.filter((field) => field.group === group.id).map((field) => (
                    <Field
                      key={field.key}
                      as={field.control === "text" || field.control === "number" ? "label" : "div"}
                      $wide={field.wide}
                    >
                      {field.control === "slider"
                        ? `${field.label}：${numberText(draft[field.key])}${field.unit ? ` ${field.unit}` : ""}`
                        : field.label}
                      {control(field)}
                      <Hint>{field.hint}</Hint>
                      <Hint>{field.fileKey} · 默认 {describeDefault(field, defaults)}</Hint>
                      {errors.has(field.key) && (
                        <ErrorText role="alert">{errors.get(field.key)}</ErrorText>
                      )}
                    </Field>
                  ))}
                </Grid>
              </PixelPanel>
            ))}
          </div>
        ))}

        <PixelPanel>
          <Actions>
            <PixelButton type="submit" $variant="bamboo" disabled={!changed || errors.size > 0}>
              保存设置
            </PixelButton>
            <PixelButton type="button" onClick={restoreDefaults}>
              恢复默认值
            </PixelButton>
            {message && <Hint role="status">{message}</Hint>}
          </Actions>
        </PixelPanel>
      </Form>
    </>
  );
}
