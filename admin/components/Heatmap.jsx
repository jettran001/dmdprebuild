// admin/components/Heatmap.jsx
import React from 'react';
import { MapContainer, TileLayer, GeoJSON } from 'react-leaflet';
import 'leaflet/dist/leaflet.css';

// Dữ liệu quốc gia (giả lập, có thể thay bằng thư viện như react-leaflet-choropleth)
const Heatmap = ({ data }) => {
    return (
        <div style={{ height: '200px' }}>
            <MapContainer center={[20, 0]} zoom={2} style={{ height: '100%' }}>
                <TileLayer
                    url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
                    attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
                />
                {/* Giả lập heatmap bằng cách hiển thị dữ liệu quốc gia */}
                {data.map(country => (
                    <GeoJSON
                        key={country.country}
                        data={country} // Thay bằng dữ liệu GeoJSON thực tế
                        style={() => ({
                            fillColor: country.value > 4000 ? '#ff0000' : country.value > 2000 ? '#ff9900' : '#00ff00',
                            weight: 2,
                            opacity: 1,
                            color: 'white',
                            fillOpacity: 0.7
                        })}
                    />
                ))}
            </MapContainer>
        </div>
    );
};

export default Heatmap;