import { useEffect, useState, useSyncExternalStore } from "react";
import styled from "styled-components";

import icon from "../assets/icon.png";
import { LogRow } from "../components/LogRow";
import {
  PanelTitle,
  PixelButton,
  PixelPanel,
  StatTile,
  StatusLamp,
  SunkenBox,
} from "../components/pixel";
import { logStore } from "../state/logStore";
import { useServer } from "../state/ServerContext";

const Hero = styled(PixelPanel)`
  display: flex;
  align-items: center;
  gap: 20px;
`;

const HeroIcon = styled.img`
  width: 96px;
  height: 96px;
  filter: drop-shadow(4px 4px 0 rgba(0, 0, 0, 0.35));
`;

const HeroText = styled.div`
  flex: 1;
  min-width: 0;
`;

const HeroTitle = styled.h1`
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0 0 6px;
  font-size: ${({ theme }) => theme.font.title};
  font-weight: 400;
  line-height: 1.25;
`;

const Muted = styled.p`
  margin: 0;
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

const Tiles = styled.div`
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
`;

const AddressRow = styled.div`
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 6px;
`;

const Address = styled.code`
  font-family: inherit;
  font-size: ${({ theme }) => theme.font.title};
  user-select: text;
`;

const Steps = styled.ol`
  margin: 0;
  padding-left: 20px;
  line-height: 2;
`;

const RecentPanel = styled(PixelPanel)`
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 120px;
`;

const RecentBox = styled(SunkenBox)`
  flex: 1;
  min-height: 0;
  padding: 8px 10px;
  overflow: hidden;
  line-height: 1.6;
`;

const Hint = styled.span`
  color: ${({ theme }) => theme.color.textMuted};
`;

const RECENT_LINES = 40;

const twoDigits = (value: number) => String(value).padStart(2, "0");

/** 运行时长每秒变化，单独做成组件，不让整个页面跟着重渲染。 */
function Uptime({ startedAtMs }: { startedAtMs: number | null }) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (startedAtMs === null) {
      return;
    }
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [startedAtMs]);

  if (startedAtMs === null) {
    return <>--:--:--</>;
  }
  const seconds = Math.max(0, Math.floor((now - startedAtMs) / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return (
    <>
      {twoDigits(hours)}:{twoDigits(minutes)}:{twoDigits(seconds % 60)}
    </>
  );
}

/** 最近的几条日志。单独订阅日志，不让总览页的其余部分跟着刷新。 */
function RecentLogs() {
  const lines = useSyncExternalStore(logStore.subscribe, logStore.getSnapshot);
  const recent = lines.slice(-RECENT_LINES).reverse();

  return (
    <RecentBox>
      {recent.length === 0 ? (
        <Hint>启动服务器后，这里会显示最新的运行记录。</Hint>
      ) : (
        recent.map((line) => <LogRow key={line.id} line={line} />)
      )}
    </RecentBox>
  );
}

export function OverviewPage() {
  const { snapshot, settings, busy, start, stop } = useServer();
  const running = snapshot?.running ?? false;
  const address = snapshot?.address ?? `${settings?.host ?? "-"}:${settings?.port ?? "-"}`;
  const maxPlayers = snapshot?.maxPlayers ?? settings?.maxPlayers ?? 0;

  return (
    <>
      <Hero>
        <HeroIcon src={icon} alt="" />
        <HeroText>
          <HeroTitle>
            <StatusLamp $on={running} />
            {running ? "服务器运行中" : "服务器已停止"}
          </HeroTitle>
          <Muted>{settings?.motd ?? ""}</Muted>
        </HeroText>
        {running ? (
          <PixelButton
            type="button"
            $variant="redstone"
            $size="large"
            disabled={busy}
            onClick={() => void stop()}
          >
            停止
          </PixelButton>
        ) : (
          <PixelButton
            type="button"
            $variant="bamboo"
            $size="large"
            disabled={busy || !settings}
            onClick={() => void start()}
          >
            启动
          </PixelButton>
        )}
      </Hero>

      <Tiles>
        <StatTile label="在线玩家">
          {snapshot?.players.length ?? 0} / {maxPlayers}
        </StatTile>
        <StatTile label="运行时长">
          <Uptime startedAtMs={snapshot?.startedAtMs ?? null} />
        </StatTile>
        <StatTile label="游戏版本">{snapshot?.gameVersion ?? "-"}</StatTile>
        <StatTile label="协议号">{snapshot?.protocolVersion ?? "-"}</StatTile>
      </Tiles>

      <PixelPanel>
        <PanelTitle>如何连接</PanelTitle>
        <AddressRow>
          <Address>{address}</Address>
          <PixelButton
            type="button"
            $size="small"
            onClick={() => void navigator.clipboard.writeText(address)}
          >
            复制
          </PixelButton>
        </AddressRow>
        <Steps>
          <li>启动 Minecraft {snapshot?.gameVersion ?? ""}，进入多人游戏</li>
          <li>选择添加服务器，填入上面的地址</li>
        </Steps>
      </PixelPanel>

      <RecentPanel>
        <PanelTitle>最近日志</PanelTitle>
        <RecentLogs />
      </RecentPanel>
    </>
  );
}
