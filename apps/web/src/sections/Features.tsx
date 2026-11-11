import styled from "styled-components";

import { PixelIcon } from "../components/PixelIcon";
import { Card, GuiPanel, Section, SectionHeading, Slot, Sunken, Tag } from "../components/pixel";
import { FEATURES, NOT_YET } from "../data/project";
import { useLocale } from "../i18n";
import { textShadow } from "../theme/bevel";

const Grid = styled.ul`
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 14px;

  @media (max-width: 860px) {
    grid-template-columns: 1fr;
  }
`;

const FeatureCard = styled(Card)`
  display: grid;
  grid-template-columns: auto 1fr;
  align-items: start;
  gap: 6px 14px;
`;

const Body = styled.div`
  display: flex;
  flex-direction: column;
  gap: 8px;

  h4 {
    font-size: ${({ theme }) => theme.font.lead};
    line-height: 1.3;
    color: ${({ theme }) => theme.color.panelText};
  }

  p {
    line-height: 1.55;
    color: ${({ theme }) => theme.color.panelText};
  }
`;

const Tags = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
`;

const Missing = styled(Sunken)`
  margin-top: 20px;
  padding: 16px 20px;

  h4 {
    margin-bottom: 8px;
    font-size: ${({ theme }) => theme.font.lead};
    line-height: 1.3;
    color: ${({ theme }) => theme.color.gold};
    ${textShadow("#3f3000")}
  }

  ul {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 4px 24px;
  }

  li {
    position: relative;
    padding-left: 20px;
    line-height: 1.55;
  }

  li::before {
    content: "";
    position: absolute;
    left: 0;
    top: 9px;
    width: 8px;
    height: 8px;
    background: ${({ theme }) => theme.status.planned};
    border: 2px solid #000000;
  }

  @media (max-width: 860px) {
    ul {
      grid-template-columns: 1fr;
    }
  }
`;

export function Features() {
  const { t } = useLocale();
  return (
    <Section id="features">
      <SectionHeading
        title={t({ zh: "现在能玩什么", en: "What works today" })}
        caption={t({ zh: "下面每一项都已经在 main 分支里", en: "Everything below is already on the main branch" })}
      />
      <GuiPanel>
        <Grid>
          {FEATURES.map((feature) => (
            <FeatureCard key={feature.icon}>
              <Slot $size={56}>
                <PixelIcon name={feature.icon} size={36} />
              </Slot>
              <Body>
                <h4>{t(feature.title)}</h4>
                <p>{t(feature.body)}</p>
                <Tags>
                  {feature.tags.map((tag) => (
                    <Tag key={tag.en}>{t(tag)}</Tag>
                  ))}
                </Tags>
              </Body>
            </FeatureCard>
          ))}
        </Grid>
        <Missing>
          <h4>{t({ zh: "还做不到的", en: "Not there yet" })}</h4>
          <ul>
            {NOT_YET.map((item) => (
              <li key={item.en}>{t(item)}</li>
            ))}
          </ul>
        </Missing>
      </GuiPanel>
    </Section>
  );
}
