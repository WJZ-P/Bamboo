import { Fragment } from "react";
import styled from "styled-components";

import { BarChart, type BarRow } from "../components/BarChart";
import { GuiPanel, GuiTitle, PanelHead, Section, SectionHeading, Sunken } from "../components/pixel";
import { CRATE_ROLES, docUrl, PLANES, REGION_RULES, type Plane } from "../data/project";
import stats from "../data/stats.generated.json";
import { formatNumber, useLocale, type Text } from "../i18n";
import { RegionSim } from "../region/RegionSim";
import { textShadow } from "../theme/bevel";

const Stack = styled.div`
  display: flex;
  flex-direction: column;
  gap: 28px;
`;

const Remark = styled.p`
  margin-top: 16px;
  line-height: 1.55;
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

// ---- 三个平面加主控 / Three planes and the coordinator ----

const PlaneColumn = styled.div`
  display: flex;
  flex-direction: column;
`;

const PlaneBox = styled(Sunken)<{ $accent: string }>`
  display: grid;
  grid-template-columns: 220px 1fr;
  align-items: center;
  gap: 4px 20px;
  padding: 12px 16px 12px 20px;
  box-shadow: inset 6px 0 0 0 ${({ $accent }) => $accent};

  @media (max-width: 760px) {
    grid-template-columns: 1fr;
  }
`;

const PlaneName = styled.div`
  h4 {
    font-size: ${({ theme }) => theme.font.lead};
    line-height: 1.3;
    color: #ffffff;
    ${textShadow()}
  }

  span {
    display: block;
    font-size: ${({ theme }) => theme.font.caption};
    line-height: 1.5;
    color: ${({ theme }) => theme.color.textMuted};
  }
`;

const PlaneBody = styled.p`
  line-height: 1.55;
  color: ${({ theme }) => theme.color.text};
`;

/** 两个平面之间的消息：往下的和往上的各一行。 / The messages between two planes: one row going down and one coming up. */
const Wire = styled.div`
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 0 28px;
  padding: 6px 12px;
  line-height: 1.4;
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

const WIRES: Record<Plane["key"], readonly [Text, Text] | null> = {
  network: [
    { zh: "▼ 每名玩家一条入站队列", en: "▼ one inbound queue per player" },
    { zh: "▲ 每名玩家一个出站缓冲", en: "▲ one outbound buffer per player" },
  ],
  simulation: [
    { zh: "▼ 票的增减、离境、卸载交还", en: "▼ ticket changes, departures, unloads" },
    { zh: "▲ 区块交付、到达、合并拆分", en: "▲ chunk deliveries, arrivals, merges and splits" },
  ],
  coordinator: [
    { zh: "▼ 带优先级的任务", en: "▼ prioritised tasks" },
    { zh: "▲ 完成", en: "▲ completions" },
  ],
  chunk: null,
};

// ---- Region ----

const RegionLayout = styled.div`
  display: grid;
  grid-template-columns: minmax(0, 1.5fr) minmax(0, 1fr);
  gap: 24px;
  align-items: start;

  @media (max-width: 900px) {
    grid-template-columns: 1fr;
  }
`;

const SimCaption = styled.figcaption`
  margin-top: 8px;
  font-size: ${({ theme }) => theme.font.caption};
  line-height: 1.5;
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

const RuleList = styled.ul`
  display: flex;
  flex-direction: column;
  gap: 10px;
`;

const Rule = styled.li`
  position: relative;
  padding-left: 22px;
  line-height: 1.5;
  color: ${({ theme }) => theme.color.panelText};

  &::before {
    content: "";
    position: absolute;
    left: 0;
    top: 8px;
    width: 10px;
    height: 10px;
    background: ${({ theme }) => theme.color.bamboo.base};
    border: 2px solid #000000;
  }
`;

const ChartFrame = styled(Sunken)`
  padding: 12px;
`;

export function Architecture() {
  const { locale, t } = useLocale();
  const accents = ["#3fa0c4", "#4f9e3a", "#b98a14", "#9a7be0"];
  const crates = [...stats.crates].sort((a, b) => b.lines - a.lines);
  const rows: BarRow[] = crates.map((crate) => ({
    key: crate.name,
    label: crate.name,
    caption: t(CRATE_ROLES[crate.name] ?? { zh: "", en: "" }),
    value: crate.lines,
    display: formatNumber(crate.lines, locale),
    detail: t({
      zh: `${crate.files} 个文件，${formatNumber(crate.tests, locale)} 个测试`,
      en: `${crate.files} files, ${formatNumber(crate.tests, locale)} tests`,
    }),
  }));

  return (
    <Section id="architecture">
      <SectionHeading
        title={t({ zh: "架构", en: "Architecture" })}
        caption={t({ zh: "世界状态各有其主，线程之间只传消息，不用锁", en: "Every piece of world state has one owner; threads pass messages and hold no locks" })}
      />
      <Stack>
        <GuiPanel>
          <PanelHead>
            <GuiTitle>{t({ zh: "三个平面加一个主控", en: "Three planes and a coordinator" })}</GuiTitle>
            <a href={docUrl("world-model-design.md")} target="_blank" rel="noopener noreferrer">
              {t({ zh: "设计稿", en: "Design document" })}
            </a>
          </PanelHead>
          <PlaneColumn>
            {PLANES.map((plane, index) => {
              const wire = WIRES[plane.key];
              return (
                <Fragment key={plane.key}>
                  <PlaneBox $accent={accents[index]}>
                    <PlaneName>
                      <h4>{t(plane.title)}</h4>
                      <span>{t(plane.runsOn)}</span>
                    </PlaneName>
                    <PlaneBody>{t(plane.body)}</PlaneBody>
                  </PlaneBox>
                  {wire && (
                    <Wire>
                      <span>{t(wire[0])}</span>
                      <span>{t(wire[1])}</span>
                    </Wire>
                  )}
                </Fragment>
              );
            })}
          </PlaneColumn>
          <Remark>
            {t({
              zh: "这是已经确认、正在分步实施的目标结构。现在的代码是一条 20 TPS 的世界线程，相当于只有一个 region；Global 和 Region 从一开始就分开了。",
              en: "This is the confirmed target, being built step by step. Today the code is one world thread at 20 TPS, the equivalent of a single region; Global and Region have been separate from the start.",
            })}
          </Remark>
        </GuiPanel>

        <GuiPanel>
          <PanelHead>
            <GuiTitle>{t({ zh: "Region：各走各的时钟", en: "Regions: each on its own clock" })}</GuiTitle>
            <span>{t({ zh: "设计中，还没实现", en: "Designed, not yet built" })}</span>
          </PanelHead>
          <RegionLayout>
            <figure>
              <ChartFrame>
                <RegionSim />
              </ChartFrame>
              <SimCaption>
                {t({
                  zh: "示意动画，不是服务端的实时数据。距离和时间都压缩过；假设三人以上的 region 过载。",
                  en: "An illustration, not live server data. Distance and time are compressed, and a region with three or more players is assumed to be overloaded.",
                })}
              </SimCaption>
            </figure>
            <RuleList>
              {REGION_RULES.map((rule) => (
                <Rule key={rule.en}>{t(rule)}</Rule>
              ))}
            </RuleList>
          </RegionLayout>
        </GuiPanel>

        <GuiPanel>
          <PanelHead>
            <GuiTitle>
              {t({ zh: `${stats.crates.length} 个 crate，依赖只朝一个方向`, en: `${stats.crates.length} crates with one-way dependencies` })}
            </GuiTitle>
            <span>{t({ zh: "手写的 Rust 行数", en: "Hand-written lines of Rust" })}</span>
          </PanelHead>
          <ChartFrame>
            <BarChart
              rows={rows}
              max={Math.max(...crates.map((crate) => crate.lines))}
              labelHeader={t({ zh: "crate", en: "Crate" })}
              valueHeader={t({ zh: "行数", en: "Lines" })}
              labelWidth={300}
            />
          </ChartFrame>
          <Remark>
            {t({
              zh: "world 不认识网络，protocol 不认识世界，两边在 game（世界线程）里汇合；game 不依赖 tokio，网络层只有 server 一个。",
              en: "`world` knows nothing of the network and `protocol` nothing of the world; they meet in `game`, the world thread. `game` does not depend on tokio, and `server` is the only network layer.",
            })}
          </Remark>
        </GuiPanel>
      </Stack>
    </Section>
  );
}
