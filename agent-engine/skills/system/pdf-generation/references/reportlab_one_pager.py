from reportlab.lib.pagesizes import LETTER
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.platypus import Paragraph, SimpleDocTemplate, Spacer


def build_one_pager(output_path: str) -> None:
    styles = getSampleStyleSheet()
    story = [
        Paragraph("Executive One-Pager", styles["Title"]),
        Spacer(1, 12),
        Paragraph("Summary", styles["Heading2"]),
        Paragraph("Revenue grew while support load declined after the workflow update.", styles["BodyText"]),
        Spacer(1, 12),
        Paragraph("Recommendation", styles["Heading2"]),
        Paragraph("Proceed with phased rollout and monthly checkpoint review.", styles["BodyText"]),
    ]
    doc = SimpleDocTemplate(output_path, pagesize=LETTER)
    doc.build(story)


if __name__ == "__main__":
    build_one_pager("/tmp/one-pager.pdf")
