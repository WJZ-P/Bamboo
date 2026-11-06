import { useMemo } from "react";
import styled from "styled-components";

import type { PlayerView } from "../api/bamboo";
import { PanelTitle, PixelPanel, PixelSlot } from "../components/pixel";
import { useServer } from "../state/ServerContext";

const Panel = styled(PixelPanel)`
  flex: 1;
  min-height: 0;
  overflow: auto;
`;

const Grid = styled.ul`
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  gap: 10px;
  margin: 0;
  padding: 0;
  list-style: none;
`;

const Card = styled.li`
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
`;

const Avatar = styled.img`
  width: 40px;
  height: 40px;
`;

const Text = styled.div`
  min-width: 0;
`;

const PlayerName = styled.div`
  font-size: ${({ theme }) => theme.font.title};
  line-height: 1.25;
`;

const Uuid = styled.div`
  overflow: hidden;
  color: ${({ theme }) => theme.color.panelTextMuted};
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: text;
`;

const Empty = styled.p`
  margin: 0;
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

const AVATAR_SIZE = 8;
const AVATAR_COLORS = ["#3c8527", "#5eac44", "#c9a227", "#b5651d", "#3b6ea5", "#8e4585"];

/** 根据 UUID 画一个左右对称的 8×8 像素头像，不需要联网获取皮肤。 */
function drawAvatar(uuid: string) {
  const canvas = document.createElement("canvas");
  canvas.width = AVATAR_SIZE;
  canvas.height = AVATAR_SIZE;
  const context = canvas.getContext("2d");
  if (!context) {
    return "";
  }

  const digits = uuid.replaceAll("-", "");
  const hexAt = (index: number) => Number.parseInt(digits[index % digits.length], 16) || 0;
  const color = AVATAR_COLORS[hexAt(0) % AVATAR_COLORS.length];

  context.fillStyle = "#2b2b2b";
  context.fillRect(0, 0, AVATAR_SIZE, AVATAR_SIZE);
  context.fillStyle = color;
  for (let y = 0; y < AVATAR_SIZE; y += 1) {
    for (let x = 0; x < AVATAR_SIZE / 2; x += 1) {
      if (hexAt(y * 4 + x + 1) % 2 === 0) {
        context.fillRect(x, y, 1, 1);
        context.fillRect(AVATAR_SIZE - 1 - x, y, 1, 1);
      }
    }
  }
  return canvas.toDataURL();
}

function PlayerCard({ player }: { player: PlayerView }) {
  const avatar = useMemo(() => drawAvatar(player.uuid), [player.uuid]);

  return (
    <Card>
      <PixelSlot>
        <Avatar src={avatar} alt="" />
      </PixelSlot>
      <Text>
        <PlayerName>{player.name}</PlayerName>
        <Uuid>{player.uuid}</Uuid>
      </Text>
    </Card>
  );
}

export function PlayersPage() {
  const { snapshot } = useServer();
  const players = snapshot?.players ?? [];

  return (
    <Panel>
      <PanelTitle>在线玩家（{players.length}）</PanelTitle>
      {players.length === 0 ? (
        <Empty>现在没有玩家在线。</Empty>
      ) : (
        <Grid>
          {players.map((player) => (
            <PlayerCard key={player.uuid} player={player} />
          ))}
        </Grid>
      )}
    </Panel>
  );
}
