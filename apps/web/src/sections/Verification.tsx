import styled from "styled-components";

import { GuiPanel, GuiTitle, PanelHead, Section, SectionHeading, Sunken } from "../components/pixel";
import { ORACLES } from "../data/project";
import stats from "../data/stats.generated.json";
import { formatNumber, useLocale, type Text } from "../i18n";
import { raisedBevel } from "../theme/bevel";

const Layout = styled.div`
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
  gap: 24px;
  align-items: start;

  @media (max-width: 900px) {
    grid-template-columns: 1fr;
  }
`;

const Steps = styled.ol`
  display: flex;
  flex-direction: column;
  gap: 12px;
  counter-reset: step;
`;

const StepItem = styled.li`
  position: relative;
  padding-left: 44px;
  line-height: 1.55;
  color: ${({ theme }) => theme.color.panelText};
  counter-increment: step;

  &::before {
    content: counter(step);
    position: absolute;
    left: 0;
    top: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border: 2px solid #000000;
    background: ${({ theme }) => theme.color.bamboo.base};
    color: #ffffff;
    line-height: 1;
    ${({ theme }) => raisedBevel(theme.color.bamboo.light, theme.color.bamboo.dark, 2)}
  }
`;

const Remark = styled.p`
  margin-top: 16px;
  line-height: 1.55;
  color: ${({ theme }) => theme.color.panelTextMuted};
`;

const Board = styled(Sunken)`
  padding: 16px;

  h4 {
    margin-bottom: 10px;
    line-height: 1.4;
    color: ${({ theme }) => theme.color.textMuted};
  }
`;

const OracleList = styled.ul`
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
`;

const Oracle = styled.li`
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 10px 2px 8px;
  border: 2px solid #000000;
  background: #1e1e1e;
  line-height: 1.5;
  ${raisedBevel("#373737", "#101010", 2)}

  /* 一个像素对勾。 / A pixel check mark. */
  &::before {
    content: "";
    width: 10px;
    height: 6px;
    margin-bottom: 4px;
    border-left: 3px solid ${({ theme }) => theme.status.done};
    border-bottom: 3px solid ${({ theme }) => theme.status.done};
    transform: rotate(-45deg);
  }
`;

const STEPS: readonly Text[] = [
  {
    zh: "出题：随机生成一批场景，比如一串方块更新、一组 ticket 的增删、一份存档数据。",
    en: "Set the questions: generate a batch of random scenes, such as a run of block updates, a set of ticket changes or a piece of save data.",
  },
  {
    zh: "官方作答：在开发机上调用官方服务端里对应的类，记下它给出的结果。",
    en: "The official answer: call the matching classes of the official server on the developer's machine and record what they return.",
  },
  {
    zh: "Bamboo 作答：同一批场景交给 Bamboo，逐项比较，有一处不同就算没完成。",
    en: "Bamboo's answer: give Bamboo the same scenes and compare item by item. A single difference means the work is not done.",
  },
];

export function Verification() {
  const { locale, t } = useLocale();
  return (
    <Section id="verification">
      <SectionHeading
        title={t({ zh: "拿官方代码当考官", en: "The official code is the examiner" })}
        caption={t({ zh: "行为对不对，不靠感觉，靠逐项比对", en: "Correct behaviour is established by comparison, not by feel" })}
      />
      <GuiPanel>
        <Layout>
          <div>
            <PanelHead>
              <GuiTitle>{t({ zh: "Oracle 对拍", en: "Oracle comparison" })}</GuiTitle>
            </PanelHead>
            <Steps>
              {STEPS.map((step) => (
                <StepItem key={step.en}>{t(step)}</StepItem>
              ))}
            </Steps>
            <Remark>
              {t({
                zh: `对拍工具在 tools/mc-bamboo-vanilla 里，其中 Java 的一侧有 ${formatNumber(stats.oracleJavaLines, locale)} 行。官方代码只在本地运行，不进仓库。`,
                en: `The comparison tool lives in tools/mc-bamboo-vanilla; its Java side is ${formatNumber(stats.oracleJavaLines, locale)} lines. Official code only runs locally and never enters the repository.`,
              })}
            </Remark>
          </div>
          <Board>
            <h4>{t({ zh: `已经有考官的 ${ORACLES.length} 个方面`, en: `${ORACLES.length} areas that already have an examiner` })}</h4>
            <OracleList>
              {ORACLES.map((oracle) => (
                <Oracle key={oracle.en}>{t(oracle)}</Oracle>
              ))}
            </OracleList>
          </Board>
        </Layout>
      </GuiPanel>
    </Section>
  );
}
