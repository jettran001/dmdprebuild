// admin/components/Chart.jsx
import React from 'react';
import { Bar } from 'react-chartjs-2';
import { Chart as ChartJS, CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend } from 'chart.js';

ChartJS.register(CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend);

const Chart = ({ title, data }) => {
    const chartData = {
        labels: data.map(item => item.name),
        datasets: [
            {
                label: title,
                data: data.map(item => item.value),
                backgroundColor: 'rgba(75, 192, 192, 0.6)',
            }
        ]
    };

    return (
        <div>
            <h4>{title}</h4>
            <Bar data={chartData} />
        </div>
    );
};

export default Chart;