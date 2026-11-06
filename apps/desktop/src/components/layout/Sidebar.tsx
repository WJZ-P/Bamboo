import styled from "styled-components";

import { useServer } from "../../state/ServerContext";
import { PixelButton } from "../pixel";

export type PageId = "overview" | "console" | "players" | "settings";

const PAGES: { id: PageId; label: string }[] = [
  { id: "overview", label: "总览" },
  { id: "console", label: "控制台" },
  { id: "players", label: "玩家" },
  { id: "settings", label: "设置" },
];

const Nav = styled.nav`
  display: flex;
  flex: none;
  flex-direction: column;
  gap: 8px;
  width: 168px;
  padding: 14px 12px;
  border-right: 4px solid ${({ theme }) => theme.color.black};
  background: rgba(0, 0, 0, 0.45);
`;

const NavButton = styled(PixelButton)`
  width: 100%;
  padding: 7px 14px;
  font-size: ${({ theme }) => theme.font.nav};
  line-height: 24px;
  text-align: left;
`;

const Footer = styled.div`
  margin-top: auto;
  color: ${({ theme }) => theme.color.textMuted};
  font-size: ${({ theme }) => theme.font.small};
  line-height: 1.6;
`;

interface SidebarProps {
  page: PageId;
  onSelect: (page: PageId) => void;
}

export function Sidebar({ page, onSelect }: SidebarProps) {
  const { snapshot } = useServer();

  return (
    <Nav aria-label="页面导航">
      {PAGES.map(({ id, label }) => (
        <NavButton
          key={id}
          type="button"
          $variant={id === page ? "bamboo" : "stone"}
          aria-current={id === page ? "page" : undefined}
          onClick={() => onSelect(id)}
        >
          {label}
        </NavButton>
      ))}
      <Footer>
        Minecraft {snapshot?.gameVersion ?? "-"}
        <br />
        协议 {snapshot?.protocolVersion ?? "-"}
      </Footer>
    </Nav>
  );
}
