import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import styled from "styled-components";

import type { PerformancePreset, ServerSettings } from "../api/bamboo";
import {
  CollapsibleGroup,
  GroupTitle,
  PanelTitle,
  PixelButton,
  PixelCollapsible,
  PixelInput,
  PixelPanel,
  PixelSelect,
  PixelSlider,
  PixelTextarea,
} from "../components/pixel";
import {
  describeDefault,
  FIELDS,
  fieldError,
  GROUPS,
  type NumberField,
  SECTIONS,
  type SelectField,
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

const Field = styled.div<{ $wide?: boolean }>`
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

/** 性能分组下面的一键预设区域，比外面的卡片浅一层。 */
const PresetPanel = styled.div`
  margin-top: 12px;
  padding: 12px 14px;
  border: 2px solid ${({ theme }) => theme.color.panelShade};
  background: rgba(0, 0, 0, 0.05);
`;

const PresetTitle = styled.h3`
  margin: 0 0 6px;
  font-size: ${({ theme }) => theme.font.body};
  font-weight: 400;
`;

/** 确认框铺在整页上面，挡住下面的操作。 */
const Overlay = styled.div`
  position: fixed;
  inset: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.55);
`;

const ConfirmBox = styled(PixelPanel)`
  width: min(560px, 100%);
  max-height: 80vh;
  overflow: auto;
`;

const DifferenceList = styled.ul`
  margin: 0 0 12px;
  padding-left: 22px;
  line-height: 1.7;
`;

/** 一键预设会改的五个字段；保存成功后只同步这几项到草稿。 */
const PRESET_KEYS = [
  "entityThreads",
  "maxEntityCollisions",
  "mobThrottleDistance",
  "mobSleepDistance",
  "mobWakeupsScale",
] as const satisfies readonly (keyof ServerSettings)[];

/** 输入框里显示的数字。NaN 表示正在编辑或者填了非数字，显示成空。 */
const numberText = (value: number) => (Number.isNaN(value) ? "" : String(value));

export function SettingsPage() {
  const { settings, defaults, presets, snapshot, updateSettings, applyPerformancePreset } =
    useServer();
  const [draft, setDraft] = useState<ServerSettings | null>(settings);
  /** 数字输入框正在编辑的原文，只在当前这一轮编辑里盖住草稿的数值。 */
  const [editing, setEditing] = useState<Partial<Record<keyof ServerSettings, string>>>({});
  const [revealSecret, setRevealSecret] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [confirmingPreset, setConfirmingPreset] = useState(false);

  useEffect(() => {
    // 设置第一次到达时填草稿；之后 settings 变化（普通保存或一键预设）不覆盖还没保存的编辑。
    // Fill the draft when settings first arrive; later changes never overwrite unsaved edits.
    setDraft((current) => current ?? settings);
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
      setEditing({});
      setMessage(snapshot?.running ? "已保存，重启服务器后生效" : "已保存");
    } catch (reason) {
      setMessage(String(reason));
    }
  };

  /**
   * 以上次保存的设置为基准套用预设，只把这五项同步到草稿；
   * 界面上其它未保存的修改留在草稿里，不会写进文件。
   */
  const applyPreset = async (preset: PerformancePreset, applied: string) => {
    try {
      const saved = await applyPerformancePreset(preset);
      setDraft((current) => (current ? { ...current, ...preset } : saved));
      setEditing((current) => {
        const next = { ...current };
        for (const key of PRESET_KEYS) {
          delete next[key];
        }
        return next;
      });
      setMessage(snapshot?.running ? `${applied}，重启服务器后生效` : applied);
    } catch (reason) {
      setMessage(String(reason));
    }
  };

  const enablePerformance = async () => {
    if (!presets) {
      return;
    }
    setConfirmingPreset(false);
    await applyPreset(presets.recommended, "已启用性能模式，推荐值已写入 bamboo.toml");
  };

  const restoreExact = () => {
    if (!presets) {
      return;
    }
    void applyPreset(presets.exact, "已恢复原版精确模式");
  };

  const control = (field: SettingField): ReactNode => {
    const invalid = errors.has(field.key);
    const disabled = field.disabled?.(draft) ?? false;

    switch (field.control) {
      case "text":
      case "password": {
        // 多行只给文本项用，密码框永远是单行。
        // Only plain text fields can be multiline; password stays single-line.
        const multiline = field.control === "text" && field.multiline === true;
        return (
          <Row>
            {multiline ? (
              <PixelTextarea
                aria-label={field.label}
                value={draft[field.key]}
                disabled={disabled}
                aria-invalid={invalid}
                spellCheck={false}
                onChange={(event) => setValue(field.key, event.target.value)}
              />
            ) : (
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
            )}
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
      }
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
      case "select":
        return (
          <PixelSelect
            aria-label={field.label}
            value={draft[field.key]}
            disabled={disabled}
            aria-invalid={invalid}
            onChange={(event) =>
              // 原生下拉只给出字符串，这里收窄回这个字段自己的取值类型。
              // A native select only yields a string; narrow it to this field's type.
              setValue(field.key, event.target.value as ServerSettings[SelectField["key"]])
            }
          >
            {field.options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </PixelSelect>
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
        {SECTIONS.map((section) => {
          const groups = GROUPS.filter((group) => group.section === section.id);
          const groupIds = new Set(groups.map((group) => group.id));
          // 这张卡片里有几项校验不过：有的话强制展开并在标题栏标出来，免得「保存」按钮
          // 灰掉却找不到原因。
          // Invalid fields inside this card: if any, force it open and flag it in the
          // header, so a greyed-out save button never has a hidden cause.
          const invalid = FIELDS.filter(
            (field) => groupIds.has(field.group) && errors.has(field.key),
          ).length;
          return (
            <PixelCollapsible
              key={section.id}
              title={section.title}
              hint={section.hint}
              forceOpen={invalid > 0}
              badge={invalid > 0 ? `${invalid} 项有误` : undefined}
            >
              {groups.map((group) => (
                <CollapsibleGroup key={group.id}>
                  <GroupTitle>{group.title}</GroupTitle>
                  <Lead>{group.hint}</Lead>
                  <Grid>
                    {FIELDS.filter((field) => field.group === group.id).map((field) => (
                      // 不用 <label> 包整格：点提示文字时浏览器会把焦点送回输入框，点空白处取消不了聚焦。
                      // 输入框自己带 aria-label。
                      // Not a wrapping <label>: clicking the hints would send focus back to the
                      // input, so clicking elsewhere in the cell could never blur it. Inputs carry
                      // their own aria-label.
                      <Field
                        key={field.key}
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
                  {group.id === "performance" && (
                    <PresetPanel>
                      <PresetTitle>一键性能模式</PresetTitle>
                      <Lead>
                        把最吃 CPU 的几项一次写好：实体并行线程数设成本机逻辑核数，
                        挤压上限 8、生物降频 32 格、生物休眠 64 格、唤醒倍率 1。
                        点之前会先列出和官方逐位一致模式的差别。
                      </Lead>
                      <Actions>
                        <PixelButton
                          type="button"
                          $variant="bamboo"
                          disabled={!presets}
                          onClick={() => setConfirmingPreset(true)}
                        >
                          一键性能模式
                        </PixelButton>
                        <PixelButton type="button" disabled={!presets} onClick={restoreExact}>
                          恢复原版精确模式
                        </PixelButton>
                        {presets && (
                          <Hint>
                            推荐：{presets.recommended.entityThreads} 条实体线程
                          </Hint>
                        )}
                      </Actions>
                    </PresetPanel>
                  )}
                </CollapsibleGroup>
              ))}
            </PixelCollapsible>
          );
        })}

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

      {confirmingPreset && presets && (
        <Overlay>
          <ConfirmBox
            role="dialog"
            aria-modal="true"
            aria-labelledby="performance-confirm-title"
          >
            <PanelTitle id="performance-confirm-title">启用一键性能模式？</PanelTitle>
            <Lead>会把推荐值写进 bamboo.toml。相对官方逐位一致的模式，会有这些差别：</Lead>
            <DifferenceList>
              <li>生物反应慢一点：32 格外的生物每 2–4 刻才决策一次，反应晚 0.1–0.2 秒。</li>
              <li>远处生物休眠：64 格外的生物基本不动，每 20 刻才完整 tick 一次。</li>
              <li>64 格外的刷怪塔效率会下降：靠生物自己走进陷阱的农场变慢。</li>
              <li>挤成一团的生物散开慢：每只每刻只处理前 8 次推挤。</li>
              <li>并行时实体看到的是这一刻开始时的状态，物品合并最多晚一刻。</li>
            </DifferenceList>
            <Lead>
              实体并行线程数会设成这台电脑的 {presets.recommended.entityThreads} 条逻辑线程。
              服务器正在运行时，写入的配置要到下次启动才生效。
            </Lead>
            <Actions>
              <PixelButton
                type="button"
                $variant="bamboo"
                onClick={() => void enablePerformance()}
              >
                确认启用
              </PixelButton>
              <PixelButton type="button" onClick={() => setConfirmingPreset(false)}>
                取消
              </PixelButton>
            </Actions>
          </ConfirmBox>
        </Overlay>
      )}
    </>
  );
}
