import styled from "styled-components";

import { Card, GuiPanel, GuiTitle, PanelHead, Section, SectionHeading, StatusBadge, Sunken } from "../components/pixel";
import { docUrl, MILESTONES, STATUS_LABEL, WORLD_MODEL_STEPS, type Milestone } from "../data/project";
import stats from "../data/stats.generated.json";
import { formatNumber, useLocale, type Text } from "../i18n";
import { raisedBevel, textShadow } from "../theme/bevel";

const StatGrid = styled.ul`
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  gap: 12px;
  margin-bottom: 28px;

  @media (max-width: 1020px) {
    grid-template-columns: repeat(3, 1fr);
  }

  @media (max-width: 520px) {
    grid-template-columns: repeat(2, 1fr);
  }
`;

const StatTile = styled(Sunken).attrs({ as: "li" })`
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 14px 8px 12px;
  text-align: center;

  strong {
    font-weight: normal;
    font-size: ${({ theme }) => theme.font.heading};
    line-height: 1.2;
    color: ${({ theme }) => theme.color.gold};
    ${textShadow("#3f3000", 3)}
  }

  span {
    line-height: 1.4;
    color: ${({ theme }) => theme.color.text};
  }

  small {
    font-size: ${({ theme }) => theme.font.caption};
    line-height: 1.4;
    color: ${({ theme }) => theme.color.textMuted};
  }
`;

const MilestoneGrid = styled.ol`
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 14px;

  @media (max-width: 1020px) {
    grid-template-columns: repeat(2, 1fr);
  }

  @media (max-width: 640px) {
    grid-template-columns: 1fr;
  }
`;

const CardHead = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
`;

const Stamp = styled.span`
  display: flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 52px;
  height: 52px;
  border: 2px solid #000000;
  background: ${({ theme }) => theme.color.stone.base};
  color: #ffffff;
  font-size: ${({ theme }) => theme.font.lead};
  line-height: 1;
  ${({ theme }) => raisedBevel(theme.color.stone.light, theme.color.stone.dark, 2)}
  ${textShadow()}
`;

const CardTitle = styled.h4`
  font-size: ${({ theme }) => theme.font.lead};
  line-height: 1.25;
  color: ${({ theme }) => theme.color.panelText};
`;

const CardBody = styled.p`
  line-height: 1.55;
  color: ${({ theme }) => theme.color.panelText};
`;

const Meter = styled.div`
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: auto;
  color: ${({ theme }) => theme.color.panelText};
  white-space: nowrap;
`;

/** 经验条那样的分段进度条，每一段是一步。 / A segmented bar like the experience bar; each segment is one step. */
const MeterTrack = styled.div`
  display: flex;
  flex: 1;
  gap: 2px;
  padding: 2px;
  border: 2px solid #000000;
  background: #101010;
`;

const MeterCell = styled.span<{ $filled: boolean }>`
  flex: 1;
  height: 10px;
  background: ${({ theme, $filled }) => ($filled ? theme.status.done : "#2f2f2f")};
  box-shadow: ${({ $filled }) => ($filled ? "inset 0 3px 0 0 rgba(255, 255, 255, 0.4)" : "none")};
`;

const LowerPanel = styled(GuiPanel)`
  margin-top: 28px;
`;

const StepList = styled.ol`
  display: flex;
  flex-direction: column;
`;

const StepRow = styled.li`
  display: grid;
  grid-template-columns: 40px minmax(140px, 220px) 132px 1fr;
  align-items: center;
  gap: 4px 14px;
  padding: 9px 4px;
  border-top: 2px solid rgba(0, 0, 0, 0.15);
  color: ${({ theme }) => theme.color.panelText};

  &:first-child {
    border-top: none;
  }

  > span:nth-child(3) {
    justify-self: start;
  }

  @media (max-width: 860px) {
    grid-template-columns: 40px 1fr auto;

    p {
      grid-column: 2 / -1;
    }
  }
`;

const StepIndex = styled(Stamp)`
  width: 36px;
  height: 36px;
  font-size: ${({ theme }) => theme.font.body};
`;

const StepTitle = styled.h4`
  line-height: 1.3;
`;

const StepNote = styled.p`
  line-height: 1.5;
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(to) - Date.parse(from)) / 86_400_000) + 1;
}

function MilestoneCard({ milestone }: { milestone: Milestone }) {
  const { t } = useLocale();
  const progress = milestone.progress;
  return (
    <Card>
      <CardHead>
        <Stamp>{milestone.id}</Stamp>
        <div>
          <CardTitle>{t(milestone.title)}</CardTitle>
          <StatusBadge status={milestone.status} label={t(STATUS_LABEL[milestone.status])} />
        </div>
      </CardHead>
      <CardBody>{t(milestone.summary)}</CardBody>
      {progress && (
        <Meter>
          <MeterTrack aria-hidden="true">
            {Array.from({ length: progress.total }, (_, index) => (
              <MeterCell key={index} $filled={index < progress.done} />
            ))}
          </MeterTrack>
          {t({ zh: `${progress.done} / ${progress.total} 步`, en: `${progress.done} of ${progress.total} steps` })}
        </Meter>
      )}
    </Card>
  );
}

export function Progress() {
  const { locale, t } = useLocale();
  const days = daysBetween(stats.firstCommitDate, stats.lastCommitDate);
  const tiles: readonly { value: string; label: Text; note?: Text }[] = [
    { value: formatNumber(stats.commits, locale), label: { zh: "次提交", en: "commits" } },
    {
      value: formatNumber(stats.rustLines, locale),
      label: { zh: "行 Rust", en: "lines of Rust" },
      note: { zh: "不含生成的数据表", en: "excluding generated data tables" },
    },
    { value: formatNumber(stats.tests, locale), label: { zh: "个测试", en: "tests" } },
    { value: String(stats.crates.length), label: { zh: "个 crate", en: "crates" } },
    { value: "44", label: { zh: "条命令", en: "commands" } },
    { value: "122", label: { zh: "种物品组件", en: "item component types" } },
  ];

  return (
    <Section id="progress">
      <SectionHeading
        title={t({ zh: "项目进度", en: "Project progress" })}
        caption={t({
          zh: `统计于提交 ${stats.commit} · ${stats.lastCommitDate} · 开工第 ${days} 天`,
          en: `Counted at commit ${stats.commit} · ${stats.lastCommitDate} · day ${days} of the project`,
        })}
      />

      <StatGrid>
        {tiles.map((tile) => (
          <StatTile key={tile.label.en}>
            <strong>{tile.value}</strong>
            <span>{t(tile.label)}</span>
            {tile.note && <small>{t(tile.note)}</small>}
          </StatTile>
        ))}
      </StatGrid>

      <GuiPanel>
        <PanelHead>
          <GuiTitle>{t({ zh: "里程碑", en: "Milestones" })}</GuiTitle>
          <span>{t({ zh: "目标只有一个版本：Minecraft Java 26.3", en: "A single target version: Minecraft Java 26.3" })}</span>
        </PanelHead>
        <MilestoneGrid>
          {MILESTONES.map((milestone) => (
            <MilestoneCard key={milestone.id} milestone={milestone} />
          ))}
        </MilestoneGrid>
      </GuiPanel>

      <LowerPanel>
        <PanelHead>
          <GuiTitle>{t({ zh: "世界模型改造：九步", en: "The world-model rework in nine steps" })}</GuiTitle>
          <a href={docUrl("world-model-design.md")} target="_blank" rel="noopener noreferrer">
            {t({ zh: "设计稿", en: "Design document" })}
          </a>
        </PanelHead>
        <StepList>
          {WORLD_MODEL_STEPS.map((step) => (
            <StepRow key={step.index}>
              <StepIndex>{step.index}</StepIndex>
              <StepTitle>{t(step.title)}</StepTitle>
              <StatusBadge status={step.status} label={t(STATUS_LABEL[step.status])} />
              <StepNote>{t(step.note)}</StepNote>
            </StepRow>
          ))}
        </StepList>
      </LowerPanel>
    </Section>
  );
}

