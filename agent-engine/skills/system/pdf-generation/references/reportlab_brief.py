from reportlab.lib import colors
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle, getSampleStyleSheet
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer, Table, TableStyle


def build_pdf(output_path: str) -> None:
    doc = SimpleDocTemplate(output_path, pagesize=A4, topMargin=48, bottomMargin=48)
    styles = getSampleStyleSheet()

    title_style = ParagraphStyle(
        "CustomTitle",
        parent=styles["Title"],
        fontName="Helvetica-Bold",
        fontSize=20,
        leading=24,
        spaceAfter=14,
    )

    story = [
        Paragraph("Operational Strategy Brief", title_style),
        Paragraph(
            "This brief summarizes the competitive landscape, the key operational risks, and the recommended next actions.",
            styles["BodyText"],
        ),
        Spacer(1, 12),
        Paragraph("Key Findings", styles["Heading2"]),
        Paragraph("• Differentiation is strongest in workflow depth and execution quality.", styles["BodyText"]),
        Paragraph("• Pricing pressure is highest in lower-market segments.", styles["BodyText"]),
        Paragraph("• Enterprise growth depends on auditability and trust signals.", styles["BodyText"]),
        Spacer(1, 12),
        Paragraph("Competitor Snapshot", styles["Heading2"]),
    ]

    table = Table(
        [
            ["Vendor", "Strength", "Risk"],
            ["InboxDoctor", "Workflow depth", "Lower market awareness"],
            ["Lemlist", "Distribution", "Positioning overlap"],
            ["ZeroBounce", "Deliverability trust", "Broader, less specific offer"],
        ],
        repeatRows=1,
    )
    table.setStyle(
        TableStyle(
            [
                ("BACKGROUND", (0, 0), (-1, 0), colors.HexColor("#E5E7EB")),
                ("GRID", (0, 0), (-1, -1), 0.5, colors.HexColor("#CBD5E1")),
                ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
                ("ROWBACKGROUNDS", (0, 1), (-1, -1), [colors.white, colors.HexColor("#F8FAFC")]),
                ("LEFTPADDING", (0, 0), (-1, -1), 8),
                ("RIGHTPADDING", (0, 0), (-1, -1), 8),
                ("TOPPADDING", (0, 0), (-1, -1), 6),
                ("BOTTOMPADDING", (0, 0), (-1, -1), 6),
            ]
        )
    )
    story.append(table)
    story.append(Spacer(1, 12))
    story.append(Paragraph("Recommendation", styles["Heading2"]))
    story.append(
        Paragraph(
            "Concentrate messaging on operational trust, measurable outcomes, and reusable execution workflows.",
            styles["BodyText"],
        )
    )

    doc.build(story)


if __name__ == "__main__":
    build_pdf("/tmp/example_brief.pdf")
