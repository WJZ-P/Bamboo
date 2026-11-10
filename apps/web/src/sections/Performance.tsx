import styled from "styled-components";

import { BarChart, type BarRow } from "../components/BarChart";
import { GuiPanel, GuiTitle, PanelHead, Section, SectionHeading, Sunken } from "../components/pixel";
import { docUrl, ENTITY_SCALING, FINDINGS, TICK_BASELINE } from "../data/project";
import { useLocale } from "../i18n";
import { textShadow } from "../theme/bevel";

const Charts = styled.div`
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr);
  gap: 24px;

  @media (max-width: 960px) {
    grid-template-columns: 1fr;
  }
`;

const ChartFrame = styled(Sunken)`
  padding: 12px;
`;

const Legend = styled.p`
  display: flex;
  flex-wrap: wrap;
  gap: 4px 20px;
  margin: 0 6px 8px;
  line-height: 1.4;
  color: ${({ theme }) => theme.color.text};
`;

const Swatch = styled.span<{ $tick?: boolean }>`
  display: inline-block;
  width: ${({ $tick }) => ($tick ? 3 : 14)}px;
  height: 14px;
  margin-right: 8px;
  vertical-align: -2px;
  background: ${({ theme, $tick }) => ($tick ? theme.color.text : theme.chart.bar)};
`;

const Note = styled.p`
  margin-top: 10px;
  line-height: 1.55;
  color: ${({ theme }) => theme.color.panelTextMuted};

  a {
    color: ${({ theme }) => theme.color.bamboo.dark};
    box-shadow: 0 2px 0 0 ${({ theme }) => theme.color.bamboo.base};
  }
`;

const FindingGrid = styled.ul`
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 14px;
  margin-top: 24px;

  @media (max-width: 960px) {
    grid-template-columns: 1fr;
  }
`;

const FindingTile = styled(Sunken).attrs({ as: "li" })`
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 16px;

  strong {
    font-weight: normal;
    font-size: ${({ theme }) => theme.font.heading};
    line-height: 1.2;
    color: ${({ theme }) => theme.color.gold};
    ${textShadow("#3f3000", 3)}
  }

  strong small {
    margin-left: 8px;
    font-size: ${({ theme }) => theme.font.body};
    color: ${({ theme }) => theme.color.text};
    text-shadow: none;
  }

  h4 {
    font-size: ${({ theme }) => theme.font.lead};
    line-height: 1.3;
    color: #ffffff;
  }

  p {
    line-height: 1.55;
    color: ${({ theme }) => theme.color.text};
  }

  a {
    margin-top: auto;
    color: ${({ theme }) => theme.color.textMuted};
    font-size: ${({ theme }) => theme.font.caption};
    line-height: 1.5;
  }

  a:hover {
    color: #ffffff;
  }
`;

export function Performance() {
  const { t } = useLocale();

  const tickRows: BarRow[] = TICK_BASELINE.scenarios.map((scenario) => ({
    key: scenario.key,
    label: t(scenario.label),
    caption: scenario.key,
    value: scenario.p50,
    marker: scenario.p99,
    display: `${scenario.p50.toFixed(1)} ms`,
    detail: t({
      zh: `一刻的耗时：中位数 ${scenario.p50} ms，p99 ${scenario.p99} ms`,
      en: `Time per tick: median ${scenario.p50} ms, p99 ${scenario.p99} ms`,
    }),
  }));

  const slowest = ENTITY_SCALING.runs[0].ms;
  const threadRows: BarRow[] = ENTITY_SCALING.runs.map((run) => ({
    key: String(run.threads),
    label: t({ zh: `${run.threads} 条线程`, en: `${run.threads} ${run.threads === 1 ? "thread" : "threads"}` }),
    value: run.ms,
    display: `${run.ms.toFixed(2)} ms`,
    detail: t({
      zh: `比单线程快 ${(slowest / run.ms).toFixed(1)} 倍`,
      en: `${(slowest / run.ms).toFixed(1)}× the single-thread speed`,
    }),
  }));
  const best = ENTITY_SCALING.runs[ENTITY_SCALING.runs.length - 1];

  return (
    <Section id="performance">
      <SectionHeading
        title={t({ zh: "量出来的数字", en: "Measured, not guessed" })}
        caption={t({ zh: "先装剖析器，再谈优化；每个数字都能在仓库里复现", en: "Profiler first, optimisation second; every figure can be reproduced from the repository" })}
      />
      <GuiPanel>
        <Charts>
          <div>
            <PanelHead>
              <GuiTitle>{t({ zh: "一刻用掉多少时间", en: "Time spent per tick" })}</GuiTitle>
              <span>{t({ zh: "预算 50 ms", en: "Budget: 50 ms" })}</span>
            </PanelHead>
            <ChartFrame>
              <Legend>
                <span>
                  <Swatch />
                  {t({ zh: "中位数", en: "Median" })}
                </span>
                <span>
                  <Swatch $tick />
                  p99
                </span>
              </Legend>
              <BarChart
                rows={tickRows}
                max={TICK_BASELINE.budgetMs}
                labelHeader={t({ zh: "场景", en: "Scenario" })}
                valueHeader={t({ zh: "中位数", en: "Median" })}
                axis={["0", "50 ms"]}
                labelWidth={190}
              />
            </ChartFrame>
            <Note>
              {t({
                zh: "超平坦世界，假玩家压测，测量时机器上还有日常进程在跑。200 人挤在一起时，p99 用掉一刻预算的 19%。",
                en: "Flat world, fake-player load test, on a machine that was not idle. With 200 players packed together, p99 uses 19% of the tick budget.",
              })}{" "}
              <a href={docUrl(TICK_BASELINE.source)} target="_blank" rel="noopener noreferrer">
                {TICK_BASELINE.source}
              </a>
            </Note>
          </div>

          <div>
            <PanelHead>
              <GuiTitle>{t({ zh: "实体阶段：并行算，按顺序落地", en: "Entities: compute in parallel, commit in order" })}</GuiTitle>
            </PanelHead>
            <ChartFrame>
              <BarChart
                rows={threadRows}
                max={slowest}
                labelHeader={t({ zh: "线程数", en: "Threads" })}
                valueHeader={t({ zh: "整刻耗时", en: "Whole tick" })}
                axis={["0", `${slowest.toFixed(2)} ms`]}
                labelWidth={110}
              />
            </ChartFrame>
            <Note>
              {t({
                zh: `3000 个掉落物加 300 个下落方块挤在一片平地上。${best.threads} 条线程比单线程快 ${(slowest / best.ms).toFixed(1)} 倍，结果和线程数无关。生物 AI 的占比还没量，这个倍数不能外推。`,
                en: `3,000 item entities and 300 falling blocks packed on flat ground. ${best.threads} threads run ${(slowest / best.ms).toFixed(1)}× faster than one, with results independent of the thread count. Mob AI has not been measured, so this ratio does not generalise.`,
              })}{" "}
              <a href={docUrl(ENTITY_SCALING.source)} target="_blank" rel="noopener noreferrer">
                {ENTITY_SCALING.source}
              </a>
            </Note>
          </div>
        </Charts>

        <FindingGrid>
          {FINDINGS.map((finding) => (
            <FindingTile key={finding.title.en}>
              <strong>
                {finding.value}
                <small>{t(finding.unit)}</small>
              </strong>
              <h4>{t(finding.title)}</h4>
              <p>{t(finding.body)}</p>
              <a href={docUrl(finding.source)} target="_blank" rel="noopener noreferrer">
                docs/{finding.source}
              </a>
            </FindingTile>
          ))}
        </FindingGrid>
      </GuiPanel>
    </Section>
  );
}
