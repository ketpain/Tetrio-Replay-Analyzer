import sys
import os
import json
import colorsys
from PyQt5.QtWidgets import (QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, 
                             QListWidget, QPushButton, QLabel, QComboBox, QFileDialog, 
                             QHeaderView, QSplitter, QGridLayout, QFrame, QTableWidget, QTableWidgetItem,
                             QAbstractItemView, QTabWidget, QLineEdit, QScrollArea, QDialog, QFormLayout, QDoubleSpinBox)
from PyQt5.QtCore import Qt, QThread, pyqtSignal
from PyQt5.QtGui import QPainter, QColor, QPen, QFont
import numpy as np

# Define the ranges for each statistic
STAT_RANGES = {
    'PPS': (0, 4), #Pieces per second
    'APM': (0, 240), #Attacks Per Minute
    'VS Score': (0, 400), #Versus Score
    'APP': (0, 1), #Attacks Per Piece
    'DS/Piece': (0, 0.5), #Downstacks Per Piece
    'DS/Second': (0, 1), #Downstacks Per Second
    'Garbage Efficiency': (0, 0.6), #Garbage Efficiency
    'Damage Potential': (0, 8) #Damage Potential
}

def generate_distinct_colors(n):
    colors = []
    for i in range(n):
        hue = i / n
        saturation = 0.7
        value = 0.9
        rgb = colorsys.hsv_to_rgb(hue, saturation, value)
        colors.append(QColor(int(rgb[0]*255), int(rgb[1]*255), int(rgb[2]*255)))
    return colors

def normalize_stat(value, stat_name):
    min_val, max_val = STAT_RANGES[stat_name]
    if max_val == min_val:
        return 0.5
    return min(max((value - min_val) / (max_val - min_val), 0), 1)

def calculate_garbage_efficiency(pps, ds_per_second, app):
    if pps <= 0 or app <= 0:
        return 0
    return ((app*ds_per_second) / pps) * 2

def calculate_app(apm, pps):
    if pps <= 0 or apm <= 0:
        return 0
    return apm / (pps * 60)

def calculate_ds_per_piece(vs, apm, pps):
    if pps <= 0 or apm <= 0:
        return 0
    ds_per_second = (vs / 100) - (apm / 60)
    return ds_per_second / pps

def calculate_ds_per_second(vs, apm):
    return (vs / 100) - (apm / 60)

def calculate_damage_potential(pps, app, ge):
    return pps * (1 + app) * (1 + ge)

class PlayerProfile:
    def __init__(self, username):
        self.username = username
        self.games_played = 0
        self.stats = {
            'PPS': [],
            'APM': [],
            'VS Score': [],
            'APP': [],
            'DS/Piece': [],
            'DS/Second': [],
            'Garbage Efficiency': [],
            'Damage Potential': []
        }
        self.personal_bests = {stat: 0 for stat in self.stats}
        self.matchups = {}

    def add_game(self, game_stats):
        self.games_played += 1
        for stat, value in game_stats.items():
            self.stats[stat].append(value)
            if value > self.personal_bests[stat]:
                self.personal_bests[stat] = value

    def get_averages(self):
        return {stat: sum(values) / len(values) if values else 0 for stat, values in self.stats.items()}

    def get_personal_bests(self):
        return self.personal_bests

    def add_matchup(self, opponent, result):
        if opponent not in self.matchups:
            self.matchups[opponent] = {'wins': 0, 'losses': 0}
        if result == 'win':
            self.matchups[opponent]['wins'] += 1
        else:
            self.matchups[opponent]['losses'] += 1

    def get_matchup_history(self):
        return {opponent: {'ratio': wins['wins'] / (wins['wins'] + wins['losses']) if (wins['wins'] + wins['losses']) > 0 else 0, 
                           'total_games': wins['wins'] + wins['losses']}
                for opponent, wins in self.matchups.items()}

class FileProcessor(QThread):
    finished = pyqtSignal(object, str)

    def __init__(self, file_path):
        super().__init__()
        self.file_path = file_path
        self.result = None

    def run(self):
        try:
            with open(self.file_path, 'r') as f:
                data = json.load(f)

            round_stats = []
            overall_stats = {}

            if 'rounds' in data.get('replay', {}):
                for round_index, round_data in enumerate(data['replay']['rounds'], 1):
                    round_stats.append({})
                    for player_data in round_data:
                        stats = player_data['stats']
                        username = player_data['username']
                        pps = stats['pps']
                        apm = stats['apm']
                        vs = stats['vsscore']
                        app = calculate_app(apm, pps)
                        ds_per_piece = calculate_ds_per_piece(vs, apm, pps)
                        ds_per_second = calculate_ds_per_second(vs, apm)
                        garbage_efficiency = calculate_garbage_efficiency(pps, ds_per_piece, app)
                        damage_potential = calculate_damage_potential(pps, app, garbage_efficiency)

                        round_stats[-1][username] = {
                            'PPS': pps,
                            'APM': apm,
                            'VS Score': vs,
                            'APP': app,
                            'DS/Piece': ds_per_piece,
                            'DS/Second': ds_per_second,
                            'Garbage Efficiency': garbage_efficiency,
                            'Damage Potential': damage_potential
                        }

                        if username not in overall_stats:
                            overall_stats[username] = {stat: [] for stat in round_stats[-1][username]}

                        for stat, value in round_stats[-1][username].items():
                            overall_stats[username][stat].append(value)

            elif 'data' in data:
                for round_index, round_data in enumerate(data['data'], 1):
                    round_stats.append({})
                    for player_data in round_data['board']:
                        username = player_data['username']
                        for endcontext_data in data['endcontext']:
                            if endcontext_data['username'] == username:
                                stats = endcontext_data['points']
                                pps = stats['tertiary']
                                apm = stats['secondary']
                                vs = stats['extra']['vs']
                                break

                        app = calculate_app(apm, pps)
                        ds_per_piece = calculate_ds_per_piece(vs, apm, pps)
                        ds_per_second = calculate_ds_per_second(vs, apm)
                        garbage_efficiency = calculate_garbage_efficiency(pps, ds_per_piece, app)
                        damage_potential = calculate_damage_potential(pps, app, garbage_efficiency)

                        round_stats[-1][username] = {
                            'PPS': pps,
                            'APM': apm,
                            'VS Score': vs,
                            'APP': app,
                            'DS/Piece': ds_per_piece,
                            'DS/Second': ds_per_second,
                            'Garbage Efficiency': garbage_efficiency,
                            'Damage Potential': damage_potential
                        }

                        if username not in overall_stats:
                            overall_stats[username] = {stat: [] for stat in round_stats[-1][username]}

                        for stat, value in round_stats[-1][username].items():
                            overall_stats[username][stat].append(value)

            else:
                raise ValueError("Unknown replay format")

            for username in overall_stats:
                for stat in overall_stats[username]:
                    overall_stats[username][stat] = sum(overall_stats[username][stat]) / len(overall_stats[username][stat])

            self.result = (round_stats, overall_stats)
            self.finished.emit(self.result, os.path.basename(self.file_path))
        except Exception as e:
            print(f"Error processing file {self.file_path}: {str(e)}")
            self.result = None

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
        radius = min(width, height) / 2 - 60

        angles = np.linspace(0, 2*np.pi, len(self.stat_names), endpoint=False)

        painter.setPen(QPen(QColor(100, 100, 100), 1))
        for angle in angles:
            x = center_x + radius * np.cos(angle)
            y = center_y + radius * np.sin(angle)
            painter.drawLine(int(center_x), int(center_y), int(x), int(y))

        painter.setPen(QColor(200, 200, 200))
        for i, stat in enumerate(self.stat_names):
            angle = angles[i]
            x = center_x + (radius + 30) * np.cos(angle)
            y = center_y + (radius + 30) * np.sin(angle)
            
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

        colors = generate_distinct_colors(len(self.players))
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

        self.draw_legend(painter)

    def draw_legend(self, painter):
        colors = generate_distinct_colors(len(self.players))
        legend_x = 10
        legend_y = self.height() - 30
        
        for i, player in enumerate(self.players):
            painter.setPen(QPen(colors[i], 2))
            painter.setBrush(colors[i])
            painter.drawRect(legend_x, legend_y, 20, 20)
            painter.drawText(legend_x + 25, legend_y + 15, player)
            legend_x += 175

class ManualInputDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Manual Stat Input")
        layout = QVBoxLayout(self)
        
        form_layout = QFormLayout()
        
        self.pps_input = QDoubleSpinBox()
        self.pps_input.setRange(0, 10)  # Adjust if needed
        self.pps_input.setDecimals(2)
        self.pps_input.setSingleStep(0.1)
        self.pps_input.setToolTip("Typical range: 0.1 - 4.5")
        
        self.apm_input = QDoubleSpinBox()
        self.apm_input.setRange(0, 500)  # Increased to 500
        self.apm_input.setDecimals(2)
        self.apm_input.setSingleStep(1)
        self.apm_input.setToolTip("Typical range: 1 - 350")
        
        self.vs_input = QDoubleSpinBox()
        self.vs_input.setRange(0, 1000)  # Increased to 1000 to be safe
        self.vs_input.setDecimals(2)
        self.vs_input.setSingleStep(1)
        self.vs_input.setToolTip("Typical range: 1 - 500")
        
        form_layout.addRow("PPS:", self.pps_input)
        form_layout.addRow("APM:", self.apm_input)
        form_layout.addRow("VS Score:", self.vs_input)
        
        layout.addLayout(form_layout)
        
        submit_button = QPushButton("Submit")
        submit_button.clicked.connect(self.accept)
        layout.addWidget(submit_button)
        
    def get_values(self):
        return {
            'PPS': self.pps_input.value(),
            'APM': self.apm_input.value(),
            'VS Score': self.vs_input.value()
        }

class AttackDefenseSpeedChart(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.stats = {}
        self.players = []
        self.stat_names = ['APP', 'Garbage Efficiency', 'PPS', 'Damage Potential']
        self.display_names = ['Attack Power', 'Defense/Boardstate', 'Speed', 'Damage Potential']

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
        radius = min(width, height) / 2 - 60

        angles = np.linspace(0, 2*np.pi, len(self.stat_names), endpoint=False)

        painter.setPen(QPen(QColor(100, 100, 100), 1))
        for angle in angles:
            x = center_x + radius * np.cos(angle)
            y = center_y + radius * np.sin(angle)
            painter.drawLine(int(center_x), int(center_y), int(x), int(y))

        painter.setPen(QColor(200, 200, 200))
        for i, display_name in enumerate(self.display_names):
            angle = angles[i]
            x = center_x + (radius + 30) * np.cos(angle)
            y = center_y + (radius + 30) * np.sin(angle)
            
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

        colors = generate_distinct_colors(len(self.players))
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

        self.draw_legend(painter)

    def draw_legend(self, painter):
        colors = generate_distinct_colors(len(self.players))
        legend_x = 10
        legend_y = self.height() - 30
        
        for i, player in enumerate(self.players):
            painter.setPen(QPen(colors[i], 2))
            painter.setBrush(colors[i])
            painter.drawRect(legend_x, legend_y, 20, 20)
            painter.drawText(legend_x + 25, legend_y + 15, player)
            legend_x += 175

class PlayerStatsWidget(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.layout = QVBoxLayout(self)
        self.layout.setContentsMargins(0, 0, 0, 0)
        self.layout.setSpacing(0)
        self.scroll_area = QScrollArea()
        self.scroll_area.setWidgetResizable(True)
        self.scroll_content = QWidget()
        self.scroll_layout = QVBoxLayout(self.scroll_content)
        self.scroll_area.setWidget(self.scroll_content)
        self.layout.addWidget(self.scroll_area)

    def update_stats(self, stats):
        # Clear existing widgets
        for i in reversed(range(self.scroll_layout.count())): 
            self.scroll_layout.itemAt(i).widget().setParent(None)

        if not stats:  # Handle empty stats
            empty_label = QLabel("No data to display")
            empty_label.setAlignment(Qt.AlignCenter)
            self.scroll_layout.addWidget(empty_label)
            return

        players = list(stats.keys())
        stat_names = ['PPS', 'APM', 'VS Score', 'APP', 'DS/Piece', 'DS/Second', 'Garbage Efficiency']

        table = QTableWidget(len(stat_names) + 2, len(players) + 1)
        table.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        table.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        table.horizontalHeader().setVisible(False)
        table.verticalHeader().setVisible(False)

        table.setColumnWidth(0, 250)
        for col in range(1, len(players) + 1):
            table.setColumnWidth(col, 250)

        for row in range(table.rowCount()):
            table.setRowHeight(row, 30)

        colors = generate_distinct_colors(len(stats))
        for col, player in enumerate(players, start=1):
            item = QTableWidgetItem(player)
            item.setTextAlignment(Qt.AlignCenter)
            item.setBackground(colors[col-1])
            item.setForeground(QColor(255, 255, 255))
            font = item.font()
            font.setBold(True)
            font.setPointSize(12)
            item.setFont(font)
            table.setItem(0, col, item)

        for row, stat in enumerate(stat_names, start=1):
            item = QTableWidgetItem(stat)
            item.setTextAlignment(Qt.AlignCenter)
            font = item.font()
            font.setPointSize(11)
            item.setFont(font)
            table.setItem(row, 0, item)

            for col, player in enumerate(players, start=1):
                value = stats[player][stat]
                item = QTableWidgetItem(f"{value:.2f}")
                item.setTextAlignment(Qt.AlignCenter)
                font = item.font()
                font.setPointSize(11)
                item.setFont(font)
                table.setItem(row, col, item)

        winner = max(stats, key=lambda x: stats[x]['VS Score'])
        winner_row = len(stat_names) + 1
        for col, player in enumerate(players, start=1):
            item = QTableWidgetItem("WINNER" if player == winner else "")
            item.setTextAlignment(Qt.AlignCenter)
            item.setBackground(QColor('#4CAF50') if player == winner else QColor('#2b2b2b'))
            item.setForeground(QColor(255, 255, 255))
            font = item.font()
            font.setBold(True)
            font.setPointSize(12)
            item.setFont(font)
            table.setItem(winner_row, col, item)

        self.scroll_layout.addWidget(table)

        self.setStyleSheet("""
            QTableWidget { 
                background-color: #2b2b2b; 
                color: #ffffff; 
                gridline-color: #3a3a3a;
            }
            QTableWidget::item { 
                padding: 5px; 
            }
        """)

        self.update()

class ReplayAnalyzer(QMainWindow):
    def __init__(self):
        super().__init__()

        self.setWindowTitle("Tetr.io Replay Analyzer")
        self.setGeometry(100, 100, 1920, 1080)
        self.setStyleSheet("""
            QMainWindow, QWidget { background-color: #2b2b2b; color: #ffffff; }
            QTableWidget { gridline-color: #3a3a3a; }
            QHeaderView::section { background-color: #3a3a3a; }
            QComboBox, QPushButton { background-color: #3a3a3a; border: 1px solid #505050; padding: 5px; }
            QListWidget { background-color: #323232; border: 1px solid #505050; }
            QListWidget::item:selected { background-color: #4a4a4a; }
            QTabBar::tab { background-color: #3a3a3a; color: #ffffff; padding: 8px; }
            QTabBar::tab:selected { background-color: #4a4a4a; }
            QLineEdit { background-color: #3a3a3a; color: #ffffff; border: 1px solid #505050; padding: 5px; }
        """)

        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        self.layout = QHBoxLayout(self.central_widget)

        self.main_splitter = QSplitter(Qt.Horizontal)
        self.layout.addWidget(self.main_splitter)

        self.create_file_browser()
        self.create_stats_view()

        self.main_splitter.setSizes([200, 1000])

        self.all_game_data = {}
        self.current_file = None
        self.current_folder = None
        self.player_profiles = {}

    def manual_input(self):
        dialog = ManualInputDialog(self)
        if dialog.exec_():
            manual_stats = dialog.get_values()

            # Calculate derived stats
            pps = manual_stats['PPS']
            apm = manual_stats['APM']
            vs = manual_stats['VS Score']

            app = calculate_app(apm, pps)
            ds_per_piece = calculate_ds_per_piece(vs, apm, pps)
            ds_per_second = calculate_ds_per_second(vs, apm)
            garbage_efficiency = calculate_garbage_efficiency(pps, ds_per_piece, app)
            damage_potential = calculate_damage_potential(pps, app, garbage_efficiency)

            manual_stats.update({
                'APP': app,
                'DS/Piece': ds_per_piece,
                'DS/Second': ds_per_second,
                'Garbage Efficiency': garbage_efficiency,
                'Damage Potential': damage_potential
            })

            # Display the manually input stats
            self.update_stats_display({'Manual Input': manual_stats})
            self.update_graphs({'Manual Input': manual_stats})

    def create_file_browser(self):
        file_frame = QWidget()
        file_layout = QVBoxLayout(file_frame)

        self.file_list = QListWidget()
        self.file_list.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.file_list.itemSelectionChanged.connect(self.on_file_selection_changed)

        select_button = QPushButton("Select Folder")
        select_button.clicked.connect(self.select_folder)

        refresh_button = QPushButton("Refresh")
        refresh_button.clicked.connect(self.refresh_files)

        analyze_button = QPushButton("Analyze Selected")
        analyze_button.clicked.connect(self.analyze_selected_files)

        manual_input_button = QPushButton("Manual Input")
        manual_input_button.clicked.connect(self.manual_input)

        button_layout = QHBoxLayout()
        button_layout.addWidget(select_button)
        button_layout.addWidget(refresh_button)
        button_layout.addWidget(analyze_button)
        button_layout.addWidget(manual_input_button)  # Add the new button here

        file_layout.addWidget(QLabel("Replay Files"))
        file_layout.addWidget(self.file_list)
        file_layout.addLayout(button_layout)

        self.main_splitter.addWidget(file_frame)

    def create_stats_view(self):
        stats_frame = QWidget()
        stats_layout = QVBoxLayout(stats_frame)
    
        self.player_filter = QLineEdit()
        self.player_filter.setPlaceholderText("Filter players...")
        self.player_filter.textChanged.connect(self.filter_players)
    
        self.round_selector = QComboBox()
        self.round_selector.currentIndexChanged.connect(self.on_round_select)
    
        self.player_stats_widget = PlayerStatsWidget()
    
        self.radar_chart = RadarChart()
        self.attack_defense_speed_chart = AttackDefenseSpeedChart()
    
        charts_splitter = QSplitter(Qt.Horizontal)
        charts_splitter.addWidget(self.radar_chart)
        charts_splitter.addWidget(self.attack_defense_speed_chart)
    
        splitter = QSplitter(Qt.Vertical)
        splitter.addWidget(self.player_stats_widget)
        splitter.addWidget(charts_splitter)
        splitter.setSizes([200, 400])
    
        self.profile_tabs = QTabWidget()
    
        stats_layout.addWidget(QLabel("Replay Stats"))
        stats_layout.addWidget(self.player_filter)
        stats_layout.addWidget(self.round_selector)
        stats_layout.addWidget(splitter)
        stats_layout.addWidget(self.profile_tabs)
    
        self.main_splitter.addWidget(stats_frame)

        main_splitter = QSplitter(Qt.Vertical)
        main_splitter.addWidget(splitter)
        main_splitter.addWidget(self.profile_tabs)
        main_splitter.setSizes([400, 200])  # Adjust these values as needed
    
        stats_layout.addWidget(main_splitter)
    
        self.main_splitter.addWidget(stats_frame)

    def refresh_files(self):
        if self.current_folder:
            self.file_list.clear()
            for file_name in os.listdir(self.current_folder):
                if file_name.endswith('.ttrm'):
                    self.file_list.addItem(file_name)

    def select_folder(self):
        folder_path = QFileDialog.getExistingDirectory(self, "Select Folder")
        if folder_path:
            self.current_folder = folder_path
            self.refresh_files()

    def on_file_select(self, item):
        file_name = item.text()
        if file_name not in self.all_game_data:
            file_path = os.path.join(self.current_folder, file_name)
            self.processor = FileProcessor(file_path)
            self.processor.finished.connect(self.on_file_processed)
            self.processor.start()
        else:
            self.display_results(file_name)

    def on_file_selection_changed(self):
        selected_items = self.file_list.selectedItems()
        if len(selected_items) == 1:
            self.on_file_select(selected_items[0])
        elif len(selected_items) > 1:
            # Multiple files selected, prepare for analysis
            self.clear_player_profiles()
            self.player_stats_widget.update_stats({})
            self.radar_chart.set_data({})
            self.attack_defense_speed_chart.set_data({})
            self.round_selector.clear()
        else:
            # No files selected
            self.clear_player_profiles()
            self.player_stats_widget.update_stats({})
            self.radar_chart.set_data({})
            self.attack_defense_speed_chart.set_data({})
            self.round_selector.clear()

    def on_file_processed(self, data_tuple, file_name):
        round_stats, overall_stats = data_tuple
        self.all_game_data[file_name] = (round_stats, overall_stats)
        self.update_player_profiles(overall_stats)
        self.display_results(file_name)

    def update_player_profiles(self, game_data):
        for player, stats in game_data.items():
            if player not in self.player_profiles:
                self.player_profiles[player] = PlayerProfile(player)
            self.player_profiles[player].add_game(stats)

    def clear_player_profiles(self):
        self.player_profiles = {}
        while self.profile_tabs.count() > 0:
            self.profile_tabs.removeTab(0)

    def filter_players(self):
        filter_text = self.player_filter.text().lower()
        if self.current_file:
            round_stats, overall_stats = self.all_game_data[self.current_file]
            filtered_stats = {player: stats for player, stats in overall_stats.items() if filter_text in player.lower()}
            self.update_stats_display(filtered_stats)
            self.update_graphs(filtered_stats)

            # Update round selector if needed
            current_round = self.round_selector.currentIndex()
            if current_round < len(round_stats):
                filtered_round_stats = {player: stats for player, stats in round_stats[current_round].items() if filter_text in player.lower()}
                self.update_stats_display(filtered_round_stats)
                self.update_graphs(filtered_round_stats)

    def display_results(self, file_name):
        self.current_file = file_name
        round_stats, overall_stats = self.all_game_data[file_name]

        self.clear_player_profiles()  # Clear existing profiles

        self.round_selector.clear()
        self.round_selector.addItems([f"Round {i+1}" for i in range(len(round_stats))] + ["Average"])
        self.round_selector.setCurrentIndex(len(round_stats))

        self.update_stats_display(overall_stats)
        self.update_graphs(overall_stats)
        self.update_player_profiles(overall_stats)
        self.update_player_profiles_display()

    def update_player_profiles_display(self):
        while self.profile_tabs.count() > 0:
            self.profile_tabs.removeTab(0)

        for player, profile in self.player_profiles.items():
            tab = QWidget()
            layout = QVBoxLayout(tab)

            style = self.analyze_play_style(profile)
            layout.addWidget(QLabel(f"Play Style: {style}"))

            suggestions = self.get_improvement_suggestions(profile)
            suggestions_label = QLabel("Improvement Suggestions:")
            layout.addWidget(suggestions_label)
            for suggestion in suggestions:
                layout.addWidget(QLabel(f"- {suggestion}"))

            self.profile_tabs.addTab(tab, player)

        # Make the profile box smaller and adjustable
        self.profile_tabs.setMaximumHeight(300)
        self.profile_tabs.setMinimumHeight(100)

    def analyze_play_style(self, player_profile):
        averages = player_profile.get_averages()
        app = averages['APP']
        vs_apm_ratio = averages['VS Score'] / averages['APM']
        ge = averages['Garbage Efficiency']

        # Define thresholds
        app_thresholds = [0.40, 0.80]  # Low, Medium, High
        ge_thresholds = [0.1, 0.3]   # Low, Medium, High
        vs_apm_thresholds = [2.0, 2.2]  # Low, Medium, High

        # Categorize each stat
        app_category = "Low" if app < app_thresholds[0] else "High" if app >= app_thresholds[1] else "Medium"
        ge_category = "Low" if ge < ge_thresholds[0] else "High" if ge >= ge_thresholds[1] else "Medium"
        vs_apm_category = "Low" if vs_apm_ratio < vs_apm_thresholds[0] else "High" if vs_apm_ratio >= vs_apm_thresholds[1] else "Medium"

        # Determine playstyle based on the new categories
        playstyle_map = {
            ("High", "High", "High"): "Efficiency God with God-like Downstack/Boardstate",
            ("High", "High", "Medium"): "Efficient Upstacker with Average Downstack/Boardstate",
            ("High", "High", "Low"): "Efficient Upstacker with Bad Downstack/Boardstate",
            ("High", "Medium", "High"): "Efficient Upstacker with Strider Tendencies",
            ("High", "Medium", "Medium"): "Efficient Upstacker with Strider Tendencies and Average Downstack/Boardstate",
            ("High", "Medium", "Low"): "Efficient Upstacker with Strider Tendencies with Bad Downstack/Boardstate",
            ("High", "Low", "High"): "Efficient Upstacker with Superb Downstack/Boardstate",
            ("High", "Low", "Medium"): "Efficient Upstacker with Average Downstack/Boardstate",
            ("High", "Low", "Low"): "Efficient Upstacker / Opener Main / Strider",
            ("Medium", "High", "High"): "Tanker with Efficient Downstack/Boardstate, Average Upstacker",
            ("Medium", "High", "Medium"): "Tanker with Average Downstack/Boardstate, Average Upstacker",
            ("Medium", "High", "Low"): "Extreme Tanker, Average Upstacker, Highly Cheesey",
            ("Medium", "Medium", "High"): "Efficient Downstack/Boardstate, Average Upstacker",
            ("Medium", "Medium", "Medium"): "All-rounder",
            ("Medium", "Medium", "Low"): "All-rounder with Bad Downstack/Boardstate",
            ("Medium", "Low", "High"): "Efficient Downstack/Boardstate, Strider, Average Upstacker",
            ("Medium", "Low", "Medium"): "Average Upstacker, Strider, Average Downstack/Boardstate",
            ("Medium", "Low", "Low"): "Upstacker, Strider, Bad Downstack/Boardstate",
            ("Low", "High", "High"): "Efficient Downstack/Boardstate, Tanker, Bad Upstacker",
            ("Low", "High", "Medium"): "Average Downstack/Boardstate, Tanker, Bad Upstacker",
            ("Low", "High", "Low"): "Extreme Tanker, Bad Upstacker, Bad Downstack/Boardstate",
            ("Low", "Medium", "High"): "Efficient Downstacker, Average Tank Tendencies, Bad Upstacker",
            ("Low", "Medium", "Medium"): "Average Tanker, Average Downstack/Boardstate, Bad Upstacker",
            ("Low", "Medium", "Low"): "Bad Upstacker, Average Tanker, Bad Downstack/Boardstate",
            ("Low", "Low", "High"): "Efficient Downstack/Boardstate, Bad Upstacker, Strider",
            ("Low", "Low", "Medium"): "Bad Upstacker, Strider, Average Downstack/Boardstate",
            ("Low", "Low", "Low"): "Extreme Strider / Cheeser"
        }

        return playstyle_map.get((app_category, vs_apm_category, ge_category), "Unclassified Playstyle")


    def get_improvement_suggestions(self, player_profile):
        global_averages = self.calculate_global_averages()
        player_averages = player_profile.get_averages()
        suggestions = set()
        below_average_count = 0
        slightly_below_average_count = 0
    
        for stat, value in player_averages.items():
            diff = (value - global_averages[stat]) / global_averages[stat]
    
            if diff < -0.1:
                below_average_count += 1
                if stat == 'PPS':
                    suggestions.add("Work on increasing your piece placement speed.")
                elif stat == 'APM':
                    suggestions.add("Focus on improving your attack rate (Attacks Per Minute).")
                elif stat == 'VS Score':
                    suggestions.add("Try to increase your overall efficiency in sending effective garbage.")
                elif stat == 'APP':
                    suggestions.add("Work on improving your attack per piece.")
                elif stat == 'Garbage Efficiency':
                    suggestions.add("Focus on improving your defensive capabilities and downstacking efficiency.")
            elif -0.1 <= diff < -0.05:
                slightly_below_average_count += 1
    
        # Add general suggestions based on overall performance
        if not suggestions:
            if slightly_below_average_count > 0:
                suggestions.add("Your performance is close to average in most areas. Focus on gradual improvement across all aspects of your game.")
            elif below_average_count == 0 and slightly_below_average_count == 0:
                suggestions.add("Your performance is above average in all areas. To improve further, analyze top players and try to refine your techniques.")
    
        # Add suggestions for balanced improvement if needed
        if len(suggestions) <= 2:
            attack_score = (player_averages['APP'] + player_averages['APM'] / 60) / 2
            defense_score = player_averages['Garbage Efficiency']
            speed_score = player_averages['PPS']
    
            min_score = min(attack_score, defense_score, speed_score)
            if min_score == attack_score:
                suggestions.add("Consider focusing on improving your overall attacking capabilities.")
            elif min_score == defense_score:
                suggestions.add("You might benefit from enhancing your defensive play and garbage handling.")
            elif min_score == speed_score:
                suggestions.add("Improving your overall game speed could be beneficial.")
    
        # Add a general suggestion for well-rounded improvement
        if len(suggestions) < 3:
            suggestions.add("Continue to practice and aim for consistent improvement across all aspects of your gameplay.")
    
        return list(suggestions)
    
    def calculate_global_averages(self):
        all_stats = {stat: [] for stat in self.player_profiles[list(self.player_profiles.keys())[0]].stats}
        for profile in self.player_profiles.values():
            for stat, values in profile.stats.items():
                all_stats[stat].extend(values)
        return {stat: sum(values) / len(values) if values else 0 for stat, values in all_stats.items()}

    def update_stats_display(self, stats):
        self.player_stats_widget.update_stats(stats)

    def update_graphs(self, stats):
        self.radar_chart.set_data(stats)
        self.radar_chart.update()
        self.attack_defense_speed_chart.set_data(stats)
        self.attack_defense_speed_chart.update()

    def on_round_select(self, index):
        if self.current_file:
            round_stats, overall_stats = self.all_game_data[self.current_file]
            if index == self.round_selector.count() - 1:
                self.update_stats_display(overall_stats)
                self.update_graphs(overall_stats)
            else:
                self.update_stats_display(round_stats[index])
                self.update_graphs(round_stats[index])

    def analyze_selected_files(self):
        selected_items = self.file_list.selectedItems()
        if not selected_items:
            return

        self.clear_player_profiles()  # Clear existing profiles

        combined_stats = {}
        for item in selected_items:
            file_name = item.text()
            file_path = os.path.join(self.current_folder, file_name)
            if file_name not in self.all_game_data:
                processor = FileProcessor(file_path)
                processor.run()
                round_stats, overall_stats = processor.result
                self.all_game_data[file_name] = (round_stats, overall_stats)
            else:
                _, overall_stats = self.all_game_data[file_name]

            for player, stats in overall_stats.items():
                if player not in combined_stats:
                    combined_stats[player] = {stat: [] for stat in stats}
                for stat, value in stats.items():
                    combined_stats[player][stat].append(value)

        for player in combined_stats:
            for stat in combined_stats[player]:
                combined_stats[player][stat] = sum(combined_stats[player][stat]) / len(combined_stats[player][stat])

        self.update_stats_display(combined_stats)
        self.update_graphs(combined_stats)
        self.update_player_profiles(combined_stats)
        self.update_player_profiles_display()

    def keyPressEvent(self, event):
        if self.file_list.hasFocus():
            current_row = self.file_list.currentRow()
            if event.key() == Qt.Key_Up and current_row > 0:
                self.file_list.setCurrentRow(current_row - 1)
                self.on_file_select(self.file_list.currentItem())
            elif event.key() == Qt.Key_Down and current_row < self.file_list.count() - 1:
                self.file_list.setCurrentRow(current_row + 1)
                self.on_file_select(self.file_list.currentItem())
        super().keyPressEvent(event)

if __name__ == "__main__":
    app = QApplication(sys.argv)
    window = ReplayAnalyzer()
    window.show()
    sys.exit(app.exec_())