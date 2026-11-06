import { type FormEvent, useEffect, useState } from "react";
import styled from "styled-components";

import type { ServerSettings } from "../api/bamboo";
import { PanelTitle, PixelButton, PixelInput, PixelPanel, PixelSlider } from "../components/pixel";
import { useServer } from "../state/ServerContext";

const MIN_PLAYERS = 1;
const MAX_PLAYERS = 100;
const MIN_VIEW_DISTANCE = 2;
const MAX_VIEW_DISTANCE = 32;

const Panel = styled(PixelPanel)`
  flex: 1;
  min-height: 0;
  overflow: auto;
`;

const Form = styled.form`
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px 16px;
  max-width: 640px;
`;

const Field = styled.label<{ $wide?: boolean }>`
  display: flex;
  flex-direction: column;
  gap: 4px;
  grid-column: ${({ $wide }) => ($wide ? "1 / -1" : "auto")};
`;

const Hint = styled.span`
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

const Actions = styled.div`
  display: flex;
  grid-column: 1 / -1;
  align-items: center;
  gap: 12px;
`;

const isValidPort = (port: number) => Number.isInteger(port) && port >= 1 && port <= 65535;

export function SettingsPage() {
  const { settings, snapshot, updateSettings } = useServer();
  const [draft, setDraft] = useState<ServerSettings | null>(settings);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => setDraft(settings), [settings]);

  if (!draft) {
    return null;
  }

  const portValid = isValidPort(draft.port);
  const hostValid = draft.host.trim().length > 0;
  const changed = JSON.stringify(draft) !== JSON.stringify(settings);

  const edit = (patch: Partial<ServerSettings>) => {
    setDraft({ ...draft, ...patch });
    setMessage(null);
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

  return (
    <Panel>
      <PanelTitle>服务器设置</PanelTitle>
      <Form onSubmit={(event) => void handleSubmit(event)}>
        <Field>
          监听地址
          <PixelInput
            value={draft.host}
            aria-invalid={!hostValid}
            spellCheck={false}
            onChange={(event) => edit({ host: event.target.value })}
          />
          <Hint>127.0.0.1 只有本机能连，0.0.0.0 对局域网开放</Hint>
        </Field>
        <Field>
          端口
          <PixelInput
            inputMode="numeric"
            value={Number.isNaN(draft.port) ? "" : draft.port}
            aria-invalid={!portValid}
            onChange={(event) => edit({ port: Number.parseInt(event.target.value, 10) })}
          />
          <Hint>范围 1 到 65535，默认 25565</Hint>
        </Field>
        <Field $wide>
          服务器介绍
          <PixelInput
            value={draft.motd}
            maxLength={120}
            spellCheck={false}
            onChange={(event) => edit({ motd: event.target.value })}
          />
        </Field>
        <Field $wide as="div">
          最大玩家数：{draft.maxPlayers}
          <PixelSlider
            label="最大玩家数"
            min={MIN_PLAYERS}
            max={MAX_PLAYERS}
            value={draft.maxPlayers}
            onChange={(maxPlayers) => edit({ maxPlayers })}
          />
        </Field>
        <Field $wide as="div">
          视距：{draft.viewDistance} 个区块
          <PixelSlider
            label="视距"
            min={MIN_VIEW_DISTANCE}
            max={MAX_VIEW_DISTANCE}
            value={draft.viewDistance}
            onChange={(viewDistance) => edit({ viewDistance })}
          />
          <Hint>视距越大，玩家进服时要传输的数据越多</Hint>
        </Field>
        <Actions>
          <PixelButton
            type="submit"
            $variant="bamboo"
            disabled={!changed || !portValid || !hostValid}
          >
            保存设置
          </PixelButton>
          {message && <Hint role="status">{message}</Hint>}
        </Actions>
      </Form>
    </Panel>
  );
}
