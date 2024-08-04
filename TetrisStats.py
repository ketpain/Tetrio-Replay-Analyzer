import sys
import os
import json
from PyQt5.QtWidgets import (QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, 
                             QListWidget, QPushButton, QLabel, QComboBox, QTableWidget, 
                             QTableWidgetItem, QFileDialog, QHeaderView, QSplitter)
from PyQt5.QtCore import Qt, QThread, pyqtSignal
from PyQt5.QtGui import QPainter, QColor, QPen
import numpy as np

# Grabbed some of these formulas from Kerrmunism Statsheet Bot

# Define the ranges for each statistic
STAT_RANGES = {
    'PPS': (0, 4),
    'APM': (0, 240),
    'VS Score': (0, 400),
    'APP': (0, 1),
    'DS/Piece': (0, 0.5),
    'DS/Second': (0, 1),
    'Garbage Efficiency': (0, 0.6)
}

# Normalize a statistic value to a range between 0 and 1
def normalize_stat(value, stat_name):
    min_val, max_val = STAT_RANGES[stat_name]
    if max_val == min_val:
        return 0.5  # Return a middle value if the range is zero
    return min(max((value - min_val) / (max_val - min_val), 0), 1)

# Calculate garbage efficiency based on PPS, DS/Second, and APP
def calculate_garbage_efficiency(pps, ds_per_second, app):
    if pps <= 0 or app <= 0:
        return 0
    return ((app*ds_per_second) / pps) * 2

# Calculate APP (Attack Per Piece) based on APM and PPS
def calculate_app(apm, pps):
    if pps <= 0 or apm <= 0:
        return 0
    return apm / (pps * 60)

# Calculate DS/Piece based on VS Score, APM, and PPS
def calculate_ds_per_piece(vs, apm, pps):
    if pps <= 0 or apm <= 0:
        return 0
    ds_per_second = (vs / 100) - (apm / 60)
    return ds_per_second / pps

# Calculate DS/Second based on VS Score and APM
def calculate_ds_per_second(vs, apm):
    return (vs / 100) - (apm / 60)

# Thread class for processing replay files
class FileProcessor(QThread):
    finished = pyqtSignal(object, str)

    def __init__(self, file_path):
        super().__init__()
        self.file_path = file_path

    def run(self):
        try:
            with open(self.file_path, 'r') as f:
                data = json.load(f)

            round_stats = []
            overall_stats = {}

            # Check for new replay format
            if 'rounds' in data.get('replay', {}):  # Check 'replay' key safely
                # New format processing
                for round_index, round_data in enumerate(data['replay']['rounds'], 1):
                    round_stats.append({})
                    for player_data in round_data:
                        stats = player_data['stats']
                        username = player_data['username']
                        pps = stats['pps']
                        apm = stats['apm']
                        vs = stats['vsscore']

                        round_stats[-1][username] = {
                            'PPS': pps,
                            'APM': apm,
                            'VS Score': vs,
                            'APP': calculate_app(apm, pps),
                            'DS/Piece': calculate_ds_per_piece(vs, apm, pps),
                            'DS/Second': calculate_ds_per_second(vs, apm),
                            'Garbage Efficiency': calculate_garbage_efficiency(pps, calculate_ds_per_piece(vs,apm,pps), calculate_app(apm,pps)),
                        }

                        if username not in overall_stats:
                            overall_stats[username] = {stat: [] for stat in round_stats[-1][username]}

                        for stat, value in round_stats[-1][username].items():
                            overall_stats[username][stat].append(value)

            elif 'data' in data:
                # Old format processing
                for round_index, round_data in enumerate(data['data'], 1):
                    round_stats.append({})
                    for player_data in round_data['board']:
                        username = player_data['username']
                        # Extract stats from 'endcontext'
                        for endcontext_data in data['endcontext']:
                            if endcontext_data['username'] == username:
                                stats = endcontext_data['points']
                                pps = stats['tertiary']
                                apm = stats['secondary']
                                vs = stats['extra']['vs']
                                break  # Found the matching player in 'endcontext'

                        round_stats[-1][username] = {
                            'PPS': pps,
                            'APM': apm,
                            'VS Score': vs,
                            'APP': calculate_app(apm, pps),
                            'DS/Piece': calculate_ds_per_piece(vs, apm, pps),
                            'DS/Second': calculate_ds_per_second(vs, apm),
                            'Garbage Efficiency': calculate_garbage_efficiency(pps, calculate_ds_per_piece(vs,apm,pps), calculate_app(apm,pps)),
                        }

                        if username not in overall_stats:
                            overall_stats[username] = {stat: [] for stat in round_stats[-1][username]}

                        for stat, value in round_stats[-1][username].items():
                            overall_stats[username][stat].append(value)

            else:
                raise ValueError("Unknown replay format")

            # Calculate averages for overall stats
            for username in overall_stats:
                for stat in overall_stats[username]:
                    overall_stats[username][stat] = sum(overall_stats[username][stat]) / len(overall_stats[username][stat])

            self.finished.emit((round_stats, overall_stats), os.path.basename(self.file_path))
        except Exception as e:
            print(f"Error processing file {self.file_path}: {str(e)}")

# Widget for displaying the radar chart
class RadarChart(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.stats = {}
        self.players = []
        self.stat_names = ['PPS', 'APM', 'VS Score', 'APP', 'DS/Piece', 'DS/Second', 'Garbage Efficiency']

    def set_data(self, stats):
        self.stats = stats
        self.players = list(stats.keys())
        self.update()

    def paintEvent(self, event):
        if not self.stats:
            return

        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)

        width = self.width()
        height = self.height()
        center_x = width / 2
        center_y = height / 2
        radius = min(width, height) / 2 - 60  # Reduced radius to accommodate labels

        angles = np.linspace(0, 2*np.pi, len(self.stat_names), endpoint=False)

        # Draw axes
        painter.setPen(QPen(QColor(100, 100, 100), 1))
        for angle in angles:
            x = center_x + radius * np.cos(angle)
            y = center_y + radius * np.sin(angle)
            painter.drawLine(int(center_x), int(center_y), int(x), int(y))

        # Draw stat labels
        painter.setPen(QColor(200, 200, 200))
        for i, stat in enumerate(self.stat_names):
            angle = angles[i]
            x = center_x + (radius + 30) * np.cos(angle)
            y = center_y + (radius + 30) * np.sin(angle)
            
            # Adjust text alignment based on position
            flags = Qt.AlignCenter
            if x < center_x:
                flags |= Qt.AlignRight
            elif x > center_x:
                flags |= Qt.AlignLeft
            if y < center_y:
                flags |= Qt.AlignBottom
            elif y > center_y:
                flags |= Qt.AlignTop
            
            rect = painter.boundingRect(int(x-50), int(y-10), 100, 20, flags, stat)
            painter.drawText(rect, flags, stat)

        # Draw player stats
        colors = [QColor(0, 150, 255), QColor(255, 50, 50)]
        for i, (player, player_stats) in enumerate(self.stats.items()):
            painter.setPen(QPen(colors[i], 2))
            points = []
            for j, stat in enumerate(self.stat_names):
                value = normalize_stat(player_stats[stat], stat)
                x = center_x + radius * value * np.cos(angles[j])
                y = center_y + radius * value * np.sin(angles[j])
                points.append((int(x), int(y)))
            
            for j in range(len(points)):
                painter.drawLine(points[j][0], points[j][1], points[(j+1)%len(points)][0], points[(j+1)%len(points)][1])

        # Draw legend
        self.draw_legend(painter)

    def draw_legend(self, painter):
        colors = [QColor(0, 150, 255), QColor(255, 50, 50)]
        legend_x = 10
        legend_y = self.height() - 30
        
        for i, player in enumerate(self.players):
            painter.setPen(QPen(colors[i], 2))
            painter.setBrush(colors[i])
            painter.drawRect(legend_x, legend_y, 20, 20)
            painter.drawText(legend_x + 25, legend_y + 15, player)
            legend_x += 175  # Move to the next legend item

# Widget for displaying the Attack-Defense-Speed chart
class AttackDefenseSpeedChart(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.stats = {}
        self.players = []
        self.stat_names = ['APP', 'Garbage Efficiency', 'PPS']
        self.display_names = ['Attack Power', 'Defense/Boardstate', 'Speed']

    def set_data(self, stats):
        self.stats = stats
        self.players = list(stats.keys())
        self.update()

    def paintEvent(self, event):
        if not self.stats:
            return

        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)

        width = self.width()
        height = self.height()
        center_x = width / 2
        center_y = height / 2
        radius = min(width, height) / 2 - 60  # Reduced radius to accommodate labels

        angles = np.linspace(0, 2*np.pi, len(self.stat_names), endpoint=False)

        # Draw axes
        painter.setPen(QPen(QColor(100, 100, 100), 1))
        for angle in angles:
            x = center_x + radius * np.cos(angle)
            y = center_y + radius * np.sin(angle)
            painter.drawLine(int(center_x), int(center_y), int(x), int(y))

        # Draw stat labels
        painter.setPen(QColor(200, 200, 200))
        for i, display_name in enumerate(self.display_names):
            angle = angles[i]
            x = center_x + (radius + 30) * np.cos(angle)
            y = center_y + (radius + 30) * np.sin(angle)
            
            # Adjust text alignment based on position
            flags = Qt.AlignCenter
            if x < center_x:
                flags |= Qt.AlignRight
            elif x > center_x:
                flags |= Qt.AlignLeft
            if y < center_y:
                flags |= Qt.AlignBottom
            elif y > center_y:
                flags |= Qt.AlignTop
            
            rect = painter.boundingRect(int(x-50), int(y-10), 100, 20, flags, display_name)
            painter.drawText(rect, flags, display_name)

        # Draw player stats
        colors = [QColor(0, 150, 255), QColor(255, 50, 50)]
        for i, (player, player_stats) in enumerate(self.stats.items()):
            painter.setPen(QPen(colors[i], 2))
            points = []
            for j, stat in enumerate(self.stat_names):
                value = normalize_stat(player_stats[stat], stat)
                x = center_x + radius * value * np.cos(angles[j])
                y = center_y + radius * value * np.sin(angles[j])
                points.append((int(x), int(y)))
            
            for j in range(len(points)):
                painter.drawLine(points[j][0], points[j][1], points[(j+1)%len(points)][0], points[(j+1)%len(points)][1])

        # Draw legend
        self.draw_legend(painter)

    def draw_legend(self, painter):
        colors = [QColor(0, 150, 255), QColor(255, 50, 50)]
        legend_x = 10
        legend_y = self.height() - 30
        
        for i, player in enumerate(self.players):
            painter.setPen(QPen(colors[i], 2))
            painter.setBrush(colors[i])
            painter.drawRect(legend_x, legend_y, 20, 20)
            painter.drawText(legend_x + 25, legend_y + 15, player)
            legend_x += 175  # Move to the next legend item

# Main application window
class ReplayAnalyzer(QMainWindow):
    def __init__(self):
        super().__init__()

        # Set up the main window
        self.setWindowTitle("Tetr.io Replay Analyzer")
        self.setGeometry(100, 100, 1920, 1080)
        self.setStyleSheet("""
            QMainWindow, QWidget { background-color: #2b2b2b; color: #ffffff; }
            QTableWidget { gridline-color: #3a3a3a; }
            QHeaderView::section { background-color: #3a3a3a; }
            QComboBox, QPushButton { background-color: #3a3a3a; border: 1px solid #505050; padding: 5px; }
            QListWidget { background-color: #323232; border: 1px solid #505050; }
            QListWidget::item:selected { background-color: #4a4a4a; }
        """)

        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        self.layout = QHBoxLayout(self.central_widget)

        # Create a splitter for the main layout
        self.main_splitter = QSplitter(Qt.Horizontal)
        self.layout.addWidget(self.main_splitter)

        self.create_file_browser()
        self.create_stats_view()

        # Set initial sizes for the splitter
        self.main_splitter.setSizes([200, 1000])  # Adjust these values as needed

        self.all_game_data = {}
        self.current_file = None
        self.current_folder = None

    # Create the file browser widget
    def create_file_browser(self):
        file_frame = QWidget()
        file_layout = QVBoxLayout(file_frame)

        self.file_list = QListWidget()
        self.file_list.itemClicked.connect(self.on_file_select)

        select_button = QPushButton("Select Folder")
        select_button.clicked.connect(self.select_folder)

        refresh_button = QPushButton("Refresh")
        refresh_button.clicked.connect(self.refresh_files)

        button_layout = QHBoxLayout()
        button_layout.addWidget(select_button)
        button_layout.addWidget(refresh_button)

        file_layout.addWidget(QLabel("Replay Files"))
        file_layout.addWidget(self.file_list)
        file_layout.addLayout(button_layout)

        self.main_splitter.addWidget(file_frame)
    
    # Refresh the file list with the current folder contents
    def refresh_files(self):
        if self.current_folder:
            self.file_list.clear()
            for file_name in os.listdir(self.current_folder):
                if file_name.endswith('.ttrm'):
                    self.file_list.addItem(file_name)

    # Create the stats view widget
    def create_stats_view(self):
        stats_frame = QWidget()
        stats_layout = QVBoxLayout(stats_frame)

        self.round_selector = QComboBox()
        self.round_selector.currentIndexChanged.connect(self.on_round_select)

        self.stats_table = QTableWidget()
        self.stats_table.setColumnCount(3)
        self.stats_table.horizontalHeader().setSectionResizeMode(QHeaderView.Stretch)

        self.radar_chart = RadarChart()
        self.attack_defense_speed_chart = AttackDefenseSpeedChart()

        charts_splitter = QSplitter(Qt.Horizontal)
        charts_splitter.addWidget(self.radar_chart)
        charts_splitter.addWidget(self.attack_defense_speed_chart)

        splitter = QSplitter(Qt.Vertical)
        splitter.addWidget(self.stats_table)
        splitter.addWidget(charts_splitter)
        splitter.setSizes([200, 400])  # Adjust these values to change the relative sizes

        stats_layout.addWidget(QLabel("Replay Stats"))
        stats_layout.addWidget(self.round_selector)
        stats_layout.addWidget(splitter)

        # Add the stats view to the main splitter
        self.main_splitter.addWidget(stats_frame)

    # Open a file dialog to select a folder
    def select_folder(self):
        folder_path = QFileDialog.getExistingDirectory(self, "Select Folder")
        if folder_path:
            self.current_folder = folder_path
            self.refresh_files()

    # Handle file selection from the list
    def on_file_select(self, item):
        file_name = item.text()
        if file_name not in self.all_game_data:
            file_path = os.path.join(self.current_folder, file_name)
            self.processor = FileProcessor(file_path)
            self.processor.finished.connect(self.on_file_processed)
            self.processor.start()
        else:
            self.display_results(file_name)

    # Handle the processed file data
    def on_file_processed(self, data_tuple, file_name):
        round_stats, overall_stats = data_tuple
        self.all_game_data[file_name] = (round_stats, overall_stats)
        self.display_results(file_name)

    # Display the results for the selected file
    def display_results(self, file_name):
        self.current_file = file_name
        round_stats, overall_stats = self.all_game_data[file_name]

        self.round_selector.clear()
        self.round_selector.addItems([f"Round {i+1}" for i in range(len(round_stats))] + ["Average"])
        self.round_selector.setCurrentIndex(len(round_stats))

        self.update_stats_table(overall_stats)
        self.update_graph(overall_stats)

    # Handle match round selection from the dropdown
    def on_round_select(self, index):
        if self.current_file:
            round_stats, overall_stats = self.all_game_data[self.current_file]
            if index == self.round_selector.count() - 1:
                self.update_stats_table(overall_stats)
                self.update_graph(overall_stats)
            else:
                self.update_stats_table(round_stats[index])
                self.update_graph(round_stats[index])

    # Update the stats table with the current data
    def update_stats_table(self, stats):
        self.stats_table.setRowCount(7)
        stat_names = ['PPS', 'APM', 'VS Score', 'APP', 'DS/Piece', 'DS/Second', 'Garbage Efficiency']
        players = list(stats.keys())

        self.stats_table.setHorizontalHeaderLabels(["Stat"] + players[:2])

        for i, stat in enumerate(stat_names):
            self.stats_table.setItem(i, 0, QTableWidgetItem(stat))
            for j, player in enumerate(players[:2]):
                value = stats[player][stat]
                self.stats_table.setItem(i, j+1, QTableWidgetItem(f"{value:.2f}"))

        self.stats_table.resizeColumnsToContents()
        self.stats_table.resizeRowsToContents()

    # Update both charts with the current data
    def update_graph(self, stats):
        self.radar_chart.set_data(stats)
        self.radar_chart.update()
        self.attack_defense_speed_chart.set_data(stats)
        self.attack_defense_speed_chart.update()

if __name__ == "__main__":
    app = QApplication(sys.argv)
    window = ReplayAnalyzer()
    window.show()
    sys.exit(app.exec_())