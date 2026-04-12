from reportlab.lib.styles import getSampleStyleSheet
from reportlab.platypus import PageBreak, Paragraph, SimpleDocTemplate, Spacer


def build_multi_page_report(output_path: str) -> None:
    styles = getSampleStyleSheet()
    story = [
        Paragraph("Quarterly Review", styles["Title"]),
        Spacer(1, 12),
        Paragraph("This report summarizes progress, risks, and next actions.", styles["BodyText"]),
        PageBreak(),
        Paragraph("Detailed Findings", styles["Heading1"]),
        Paragraph("Operational throughput improved after the workflow redesign.", styles["BodyText"]),
        Spacer(1, 12),
        Paragraph("Risk Notes", styles["Heading2"]),
        Paragraph("The main remaining risk is timeline compression during migration.", styles["BodyText"]),
    ]
    doc = SimpleDocTemplate(output_path)
    doc.build(story)


if __name__ == "__main__":
    build_multi_page_report("/tmp/multi-page-report.pdf")
