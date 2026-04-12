from reportlab.lib import colors
from reportlab.lib.pagesizes import LETTER
from reportlab.platypus import SimpleDocTemplate, Table, TableStyle


def build_table_summary(output_path: str) -> None:
    data = [
        ["Option", "Cost", "Speed", "Risk"],
        ["A", "$24k", "Fast", "Medium"],
        ["B", "$31k", "Medium", "Low"],
        ["C", "$19k", "Fast", "High"],
    ]
    table = Table(data, repeatRows=1)
    table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, 0), colors.lightgrey),
        ("GRID", (0, 0), (-1, -1), 0.5, colors.black),
        ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
    ]))
    doc = SimpleDocTemplate(output_path, pagesize=LETTER)
    doc.build([table])


if __name__ == "__main__":
    build_table_summary("/tmp/table-summary.pdf")
